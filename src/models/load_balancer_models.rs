use std::{ops::Deref, str::FromStr, sync::{Arc, OnceLock}, thread::current};


use mongodb::bson::{doc, oid::ObjectId};
use tokio::sync::Mutex;
use crate::{models::docker_models, utils::{docker_utils::{self, create_container_instance, verify_docker_containers, LoadBalancerBehavior}, mongodb_utils::{self, DBCollection}}};

use super::docker_models::ContainerRoute;



pub struct LoadBalancer {
    pub id: String, //mongo_db_load_balancer_instance
    pub address: String,
    pub head: Arc<Mutex<usize>>,
    pub behavior: LoadBalancerBehavior,
    pub containers: Arc<Mutex<Vec<String>>>, //docker_container_id_instances
    pub validated: Arc<Mutex<bool>> //initially false to let the program know if the containers are checked
}

pub struct Container {
    pub id: String, //references the mongo_db_id_instance
    pub container_id:String, //references the docker_container_id_instance
    pub public_port: usize,
    pub last_accepted_request: Option<String>,
    pub last_replied_request: Option<String>
}


pub static LOAD_BALANCERS:OnceLock<Arc<Mutex<Vec<LoadBalancer>>>> = OnceLock::new();
pub static CONTAINERS:OnceLock<Arc<Mutex<Vec<Container>>>> = OnceLock::new();
#[derive(Debug)]
pub struct ActiveServiceDirectory {}

impl ActiveServiceDirectory{
    /// returns index of type [type usize] of the generated load_balancer
    pub async fn create_load_balancer(id:String, address:String, behavior: LoadBalancerBehavior, containers:Vec<String>)-> usize{
        let mutex = LOAD_BALANCERS.get_or_init(|| Arc::new(Mutex::new(Vec::new())));
        
        let new_load_balancer = LoadBalancer{
            id, //mongo_db_reference
            address,
            head:Arc::new(Mutex::new(0)),
            behavior,
            containers : Arc::new(Mutex::new(containers)), //docker_container_id
            validated: Arc::new(Mutex::new(false))
        };
        let mut guard = mutex.lock().await;
        guard.push(new_load_balancer);
        guard.len() - 1
    }

    pub async fn get_load_balancer_index(container_address:String) -> Option<usize>{
        let mutex = LOAD_BALANCERS.get_or_init(|| Arc::new(Mutex::new(Vec::new())));
        let mut load_balancer_index:Option<usize> = None;
        let mut guard = mutex.lock().await;
        for (index,load_balancer) in guard.iter().enumerate() {
            if load_balancer.address == container_address { //there is only 1 instance
                load_balancer_index = Some(index);
            }
        }
        load_balancer_index
    }
    ///a helper function that validates load_balancer_state
    /// also loads data from the database as a way to restore state
    pub async fn validate_load_balancer_containers(load_balancer_index:usize){
        
        let load_balancer_mutex = LOAD_BALANCERS.get().unwrap();
        let mut guard = load_balancer_mutex.lock().await;
        let current_load_balancer = &guard[load_balancer_index];
        let mut is_validated_guard = current_load_balancer.validated.lock().await;
        if *is_validated_guard == false {

            let load_balancer_id:ObjectId = ObjectId::from_str(current_load_balancer.id.as_str()).unwrap() ;
            let mongo_lb_entry = DBCollection::LOAD_BALANCERS.collection::<docker_models::LoadBalancer>().await.find_one(doc!{
                "_id": load_balancer_id 
            }, None).await.unwrap().unwrap();
            println!("mongo_lb_entry.containers{:#?}", &mongo_lb_entry.containers);
            let containers = mongo_lb_entry.containers;
            let verified_containers = verify_docker_containers(containers.clone()).await;
            let mut container_guard = current_load_balancer.containers.lock().await;
            println!("verified containers {:#?}", &verified_containers);
            DBCollection::LOAD_BALANCERS.collection::<docker_models::LoadBalancer>().await.find_one_and_update(doc!{
                "_id" : ObjectId::from_str(&mongo_lb_entry._id.to_hex()).unwrap()
            }, doc!{
                "$set": {"containers" : &verified_containers}
            }, None).await.unwrap().unwrap();

            *container_guard = verified_containers;
        }
    }

    // pub async fn create_container_instance(mongodb_container_id:String, docker_container_id:String, public_port: usize) -> usize{
    //     let containers= CONTAINERS.get_or_init(|| Arc::new(Mutex::new(Vec::new())));
        
    //     let new_container_instance = Container{
    //         id: mongodb_container_id,
    //         container_id: docker_container_id,
    //         public_port: public_port,
    //         last_accepted_request: None,
    //         last_replied_request: None,
    //     };
    //     let mut mutex = containers.lock().await;
    //     mutex.push(new_container_instance);
    //     mutex.len() - 1
    // }

    pub async fn next_container(load_balancer_index:usize)->(String, usize){
        //check if there is atleast 1 active container
        let load_balancer_mutex = LOAD_BALANCERS.get().unwrap();
        let current_load_balancer = &load_balancer_mutex.lock().await[load_balancer_index.clone()];
        let mut current_containers = current_load_balancer.containers.lock().await;
        println!("currentContainers :{:#?}", current_containers);
        if current_containers.len() == 0 {
        
            //get the image based on the path of the load_balancer;
            let container_route = DBCollection::ROUTES.collection::<ContainerRoute>().await.find_one(doc!{
                "address" : &current_load_balancer.address
            }, None).await.unwrap().unwrap();
            let docker_image = container_route.image_name;
            let load_balancer_id = current_load_balancer.id.clone();
            
            //start a container using this image
            let create_container_result = docker_utils::create_container_instance(&docker_image,&load_balancer_id, current_containers.clone()).await;

            //add container to the load_balancer
            if create_container_result.is_some() {

                current_containers.push(create_container_result.unwrap().container_id);
                let insert_containers = current_containers.clone();
                let _load_balancer_update_result = DBCollection::LOAD_BALANCERS.collection::<docker_models::LoadBalancer>().await.find_one_and_update(
                    doc!{
                        "_id": ObjectId::parse_str(&load_balancer_id.as_str()).unwrap()
                    }, doc!{
                        "$set" : {
                            "containers" : insert_containers
                        }
                    }, None).await;
            }
            
        }

        //modify head
        let mut head_mutex = current_load_balancer.head.lock().await;
        *head_mutex = (*head_mutex + 1 ) % current_containers.len();
        let next_container_docker_id = current_containers[*head_mutex].clone();
        let container = DBCollection::CONTAINERS.collection::<docker_models::Container>().await.find_one(doc!{
            "container_id": &next_container_docker_id
        }, None).await.unwrap().unwrap();
        (container.container_id, container.public_port)
    }
}
use std::{collections::HashMap, str::FromStr, sync::{Arc, OnceLock}};


use axum::response::IntoResponse;
use bollard::container::ListContainersOptions;
use hyper::StatusCode;
use mongodb::bson::{doc, oid::ObjectId};
use tokio::sync::Mutex;
use crate::{models::docker_models::{self, Route}, utils::{docker_utils::{self, create_container_instance, create_container_instance_by_load_balancer_key, try_start_container, verify_docker_containers, LoadBalancerBehavior, DOCKER_CONNECTION}, mongodb_utils::{self, DBCollection}}};




pub struct LoadBalancer {
    pub id: String, //mongo_db_load_balancer_instance
    pub address: String,
    pub head: Arc<Mutex<usize>>,
    pub behavior: LoadBalancerBehavior,
    pub containers: Arc<Mutex<Vec<String>>>, //docker_container_id_instances
    pub validated: Arc<Mutex<bool>>, //initially false to let the program know if the containers are checked
    pub automatic_container_instancing: Arc<Mutex<bool>>
}

pub struct Container {
    pub id: String, //references the mongo_db_id_instance
    pub container_id:String, //references the docker_container_id_instance
    pub public_port: usize,
    pub last_accepted_request: Option<String>,
    pub last_replied_request: Option<String>
}


pub static LOAD_BALANCERS:OnceLock<Arc<Mutex<HashMap<String, LoadBalancer>>>> = OnceLock::new();
pub static CONTAINERS:OnceLock<Arc<Mutex<HashMap<String, Container>>>> = OnceLock::new();
#[derive(Debug)]
pub struct ActiveServiceDirectory {}

impl ActiveServiceDirectory{
    /// returns index of type [type String] of the generated load_balancer
    pub async fn create_load_balancer(id:String, address:String, behavior: LoadBalancerBehavior, containers:Vec<String>, automatic_container_instancing:Option<bool>)-> String{
        let mutex = LOAD_BALANCERS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())));
        let new_load_balancer = LoadBalancer{
            id, //mongo_db_reference
            address: address.clone(),
            head:Arc::new(Mutex::new(0)),
            behavior,
            containers : Arc::new(Mutex::new(containers)), //docker_container_id
            validated: Arc::new(Mutex::new(false)),
            automatic_container_instancing:Arc::new(Mutex::new(automatic_container_instancing.unwrap_or_else(||false))),
        };
        let mut guard = mutex.lock().await;
        guard.insert(address.clone(), new_load_balancer);
        println!("[PROCESS] created a new load balancer for {}", &address);
        address
    }
    ///returns the load_balancer key of type [type Option]<[type String]>
    pub async fn get_load_balancer_key(container_address:String) -> Option<String>{
        let mutex = LOAD_BALANCERS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())));
        let mut load_balancer_index:Option<String> = None;
        let guard = mutex.lock().await;
        for (load_balancer_key,load_balancer_val) in guard.iter() {
            if load_balancer_val.address == container_address { //there is only 1 instance
                load_balancer_index = Some(load_balancer_key.clone());
            }
        }
        load_balancer_index
    }

    pub async fn remove_load_balancer(load_balancer_key:&String) -> Option<LoadBalancer>{
        let mut load_balancers_mutex = LOAD_BALANCERS.get().unwrap().lock().await;
        let load_balancer_value =  load_balancers_mutex.remove(load_balancer_key);
        return load_balancer_value; 
    }
    
    ///a helper function that validates load_balancer_state
    /// also loads data from the database as a way to restore state
    pub async fn validate_load_balancer_containers(load_balancer_key:String){
        
        let load_balancer_mutex = LOAD_BALANCERS.get().unwrap();
        let guard = load_balancer_mutex.lock().await;
        let current_load_balancer = &guard.get(&load_balancer_key).unwrap();
        let mut is_validated_guard = current_load_balancer.validated.lock().await;
        if *is_validated_guard == false {
            println!("[PROCESS] Attempting to validate lb:{}||{}", &load_balancer_key,&current_load_balancer.id);
            

            let load_balancer_id:ObjectId = ObjectId::from_str(current_load_balancer.id.as_str()).unwrap() ;
            let mongo_lb_entry = DBCollection::LOADBALANCERS.collection::<docker_models::LoadBalancer>().await.find_one(doc!{
                "_id": load_balancer_id 
            }, None).await.unwrap().unwrap();
            let containers = mongo_lb_entry.containers;
            println!("[PROCESS] Attempting to validate containers:{:#?}", &containers);
            let verified_containers = verify_docker_containers(containers.clone()).await;
            let mut container_guard = current_load_balancer.containers.lock().await;
            DBCollection::LOADBALANCERS.collection::<docker_models::LoadBalancer>().await.find_one_and_update(doc!{
                "_id" : ObjectId::from_str(&mongo_lb_entry._id.to_hex()).unwrap()
            }, doc!{
                "$set": {"containers" : &verified_containers}
            }, None).await.unwrap().unwrap();
            *is_validated_guard = true;
            *container_guard = verified_containers;
        }
    }

    ///
    pub async fn create_container_instance(mongodb_container_id:String, docker_container_id:String, public_port: usize) -> String{
        let containers= CONTAINERS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())));
        
        let new_container_instance = Container{
            id: mongodb_container_id,
            container_id: docker_container_id.clone(),
            public_port: public_port,
            last_accepted_request: None,
            last_replied_request: None,
        };
        let mut hashmap_mutex = containers.lock().await;
        println!("[PROCESS] Created container instance");
        hashmap_mutex.insert(docker_container_id.clone(), new_container_instance);
        docker_container_id
    }
    ///
    pub async fn create_container_instances(docker_container_ids:&Vec<String>){

        for docker_container_id in  docker_container_ids.iter(){
            let container_query_result = DBCollection::CONTAINERS.collection::<docker_models::Container>().await.find_one(doc!{
                "container_id": &docker_container_id
            }, None).await;

            if let Some(container) = container_query_result.unwrap(){
                ActiveServiceDirectory::create_container_instance(container._id.to_hex(), container.container_id, container.public_port).await;
            }
        };
    }

    pub async fn next_container(load_balancer_key:String)->(String, usize){
        //check if there is atleast 1 active container
        
        let current_containers = ActiveServiceDirectory::get_load_balancer_containers(&load_balancer_key).await;
        println!("currentContainers :{:#?}", current_containers);
        if current_containers.len() == 0 {
            let _create_container_result = create_container_instance_by_load_balancer_key(&load_balancer_key).await;
        }
        //modify head
        let load_balancer_mutex = LOAD_BALANCERS.get().unwrap().lock().await;
        let current_load_balancer = load_balancer_mutex.get(&load_balancer_key).unwrap();
        let mut head_mutex = current_load_balancer.head.lock().await;
        //using a new container count to reference the container vector just incase it changed
        let container_mutex = current_load_balancer.containers.lock().await;
        *head_mutex = (*head_mutex + 1 ) % container_mutex.len();
        let next_container_docker_id = container_mutex[*head_mutex].clone();
        let container = DBCollection::CONTAINERS.collection::<docker_models::Container>().await.find_one(doc!{
            "container_id": &next_container_docker_id
        }, None).await.unwrap().unwrap();
        (container.container_id, container.public_port)
    }
    
    pub async fn get_load_balancer_containers(load_balancer_key:&String)->Vec<String>{
        let load_balancer_mutex = LOAD_BALANCERS.get().unwrap().lock().await;
        let current_load_balancer = &load_balancer_mutex.get(&load_balancer_key.clone()).unwrap();
        let current_containers = current_load_balancer.containers.lock().await;
        let containers = current_containers.iter().map(|x| x.clone()).collect::<Vec<String>>();
        return containers
    }
    
    pub async fn update_load_balancer_validation(load_balancer_key:String, validation_value:bool){
        let load_balancer_mutex = LOAD_BALANCERS.get().unwrap().lock().await;
        if let Some(load_balancer_instance) = load_balancer_mutex.get(&load_balancer_key){
            let mut isValidated_mutex= load_balancer_instance.validated.lock().await;
            *isValidated_mutex = validation_value;
        }
    }

    pub async fn remove_load_balancer_container(docker_container_id: &String, load_balancer_key:&String){

        let new_containers = docker_utils::remove_container_instance(load_balancer_key, docker_container_id).await;

        let load_balancer_mutex = LOAD_BALANCERS.get().unwrap().lock().await;
        match load_balancer_mutex.get(load_balancer_key) {
           Some( load_balancer )=>{
                let mut containers_mutex = load_balancer.containers.lock().await;
                *containers_mutex = new_containers;
                println!("[PROCESS] Updated loadbalancer containers in internal memory")
           },
           None=>{println!("[PROCESS] Cannot remove non-existing load-balancer: {}", &load_balancer_key);} 
        }
    }

    pub async fn start_container_error_correction(docker_container_id: &String, load_balancer_key:&String)
    ->Result< (String, usize),impl IntoResponse>
    {

        println!("[PROCESS] Checking if docker container exists");
        let mut container_options_filter = HashMap::new();
        container_options_filter.insert("id".to_string(), vec![docker_container_id.clone()]);

        let list_container_options = ListContainersOptions {
            all: true,
            filters: container_options_filter,
            ..Default::default()
        };
        let docker = DOCKER_CONNECTION.get().unwrap();
        let container_list = docker.list_containers(Some(list_container_options)).await.unwrap();
        if container_list.len() > 0 {
            println!("[PROCESS] Container exists but cannot be started");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "Cannot create a connection to the destination").into_response())
        }else{ //cannot find container
            ActiveServiceDirectory::remove_load_balancer_container(docker_container_id, load_balancer_key).await;
            
            let created_container = docker_utils::create_container_instance_by_load_balancer_key(load_balancer_key).await;
            match created_container {
                Some(container) =>{
                    match try_start_container(&container.container_id).await {
                        Ok(_)=>{
                            println!("[PROCESS] New container via correction started");
                            Ok((container.container_id, container.public_port))
                        },
                        Err(err_string)=>{
                            println!("[ERROR] {}", err_string);
                            Err((StatusCode::INTERNAL_SERVER_ERROR, "[ERROR] Failed to start a container inside on re-attempt").into_response())
                        }
                    }
                },
                None=>{
                    Err((StatusCode::INTERNAL_SERVER_ERROR, "[ERROR] Failed to create a container instance").into_response())
                }
            }
            //return(StatusCode::OK).into_response()
        }
    }

}
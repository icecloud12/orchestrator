use std::{collections::HashMap, str::FromStr, sync::OnceLock };
use bollard::{container::{self, Config, CreateContainerOptions, ListContainersOptions, StartContainerOptions}, image::ListImagesOptions, secret::{ContainerStateStatusEnum, HostConfig, ImageSummary, PortBinding}, Docker};
use mongodb::bson::{doc, oid::ObjectId};
use rand::Rng;

use crate::models::{docker_models::{self, ContainerInsert, Image, LoadBalancer, LoadBalancerInsert, Route}, load_balancer_models::{ self, ActiveServiceDirectory, LOAD_BALANCERS}};

use super::mongodb_utils::DBCollection;
//balancer per image
pub static DOCKER_CONNECTION:OnceLock<Docker> = OnceLock::new();

pub enum LoadBalancerBehavior {
    RoundRobin
}

impl ToString for LoadBalancerBehavior {
    fn to_string(&self) -> String {
        match &self {
            &Self::RoundRobin => "round_robin".to_string()
        }
    }
}

/// returns index of load balancer
pub async fn get_load_balancer_instances(mongo_image_id:ObjectId, container_address:String) -> String{
    //check local records

    match ActiveServiceDirectory::get_load_balancer_key(container_address.clone()).await {
        Some(index)=>{
            return index;
        },
        None =>{
            //perform a database lookup for a load_balancer
            let load_balancer_result = DBCollection::LOADBALANCERS.collection::<LoadBalancer>().await.find_one(doc!{"mongo_image_reference" : mongo_image_id.clone()}, None).await.unwrap();
            match load_balancer_result {
                Some(load_balancer) => {
                    //there is an instance of the load balancer and shape of past instance
                    let index = ActiveServiceDirectory::create_load_balancer(load_balancer._id.to_hex(), container_address, LoadBalancerBehavior::RoundRobin, load_balancer.containers, Some(false)).await;
                    return index;
                },
                None => {
                    return  create_load_balancer_instance(mongo_image_id, container_address).await
                }
            }
        }
    }
    
    

    
}
///returns load_balancer_key : [type String]
pub async fn create_load_balancer_instance(mongo_image_id:ObjectId, container_address:String) -> String{
   
    let doc: LoadBalancerInsert = LoadBalancerInsert{
        mongo_image_reference: mongo_image_id.clone(),
        head: 0,
        behavior: LoadBalancerBehavior::RoundRobin.to_string(),
        containers: vec![],
    };
    let create_result: mongodb::results::InsertOneResult = DBCollection::LOADBALANCERS.collection::<LoadBalancerInsert>().await.insert_one(doc, None).await.unwrap();
    ActiveServiceDirectory::create_load_balancer(create_result.inserted_id.as_object_id().unwrap().to_hex(), container_address, LoadBalancerBehavior::RoundRobin, vec![], Some(false)).await
    
}
/// updates the load_balancer of the new container created
/// 
/// returns [type Option]<([type String],[type usize])> as (docker_container_id, container_public_port)
pub async fn create_container_instance (mongo_image:&ObjectId, load_balancer_id:&String, mut current_containers:Vec<String>)
    -> Option<load_balancer_models::Container>{
    println!("[PROCESS] Fetching image {:#?}", mongo_image);
    let docker_image = DBCollection::IMAGES.collection::<Image>().await.find_one(doc!{
        "_id": mongo_image
    }, None).await.unwrap().unwrap().docker_image_id;
    let docker_image_exist = check_if_docker_image_exist(&docker_image).await;
    println!("[PROCESS] Creating container instance with image{}",&docker_image);
    
    if  docker_image_exist{
        
        let route_find_result = DBCollection::ROUTES.collection::<Route>().await.find_one(doc! {"mongo_image" : &mongo_image}, None).await.unwrap().unwrap();
        let container_port = route_find_result.exposed_port;

        let starting_port = std::env::var("STARTING_PORT").unwrap().parse::<usize>().unwrap();

        let ending_port = std::env::var("ENDING_PORT").unwrap().parse::<usize>().unwrap();
        
        let local_port = rand::thread_rng().gen_range(starting_port..ending_port);

        let docker = DOCKER_CONNECTION.get().unwrap();
        let mut port_binding = HashMap::new();
        port_binding.insert(format!("{}/tcp",container_port), Some(vec![PortBinding{
            host_port: Some(local_port.clone().to_string()),
            host_ip: Some("0.0.0.0".to_string())
        }]));
        let options = Some(CreateContainerOptions::<String>{..Default::default() });
        let host_config:HostConfig = HostConfig {
            port_bindings : Some(port_binding),
            ..Default::default()
        };
        let config = Config {
            image: Some(docker_image.clone()),
            host_config: Some(host_config),
            ..Default::default()
        };
        let image_result = DBCollection::IMAGES.collection::<Image>().await.find_one(doc!{
            "docker_image_id": docker_image
        }, None).await.unwrap();
        match image_result {
            Some(image)=>{
                let create_container_result = docker.create_container( options, config).await.unwrap();
                let doc = ContainerInsert { 
                    mongo_image_reference: image._id.clone(), 
                    container_id: create_container_result.id.clone(), 
                    public_port: local_port.clone()
                };
                current_containers.push(create_container_result.id.clone());
                let _load_balancer_update_result = DBCollection::LOADBALANCERS.collection::<docker_models::LoadBalancer>().await.find_one_and_update(
                    doc!{"_id": ObjectId::from_str(&load_balancer_id.as_str()).unwrap()},
                    doc!{
                        "$set": {"containers": current_containers}
                }, None).await;
                let container_insert_result = DBCollection::CONTAINERS.collection::<ContainerInsert>().await.insert_one(doc, None).await.unwrap();
                
                let container = load_balancer_models::Container{
                    id: container_insert_result.inserted_id.as_object_id().unwrap().to_hex(),
                    container_id: create_container_result.id.clone(),
                    public_port: local_port,
                    last_accepted_request: None,
                    last_replied_request: None,
                };
                println!("[PROCESS] created_container model");
              
                return Some(container);
            },
            None=>{None}
        }
        
    }else{
        println!("[PROCESS] Image [{}] does not exist",&docker_image);
        return None
    }
    
}

pub async fn create_container_instance_by_load_balancer_key(load_balancer_key:&String)->Option<load_balancer_models::Container>{
    println!("[PROCESS] Creating LoadBalancer by key");
    let load_balancer_mutex = LOAD_BALANCERS.get().unwrap().lock().await;
    let load_balancer = load_balancer_mutex.get(load_balancer_key).unwrap();
    let load_balancer_id = &load_balancer.id;
    let mongo_load_balancer = DBCollection::LOADBALANCERS.collection::<docker_models::LoadBalancer>().await.find_one(doc!{"_id": ObjectId::from_str(&load_balancer_id.as_str()).unwrap()}, None).await.unwrap().unwrap();
    let current_containers = mongo_load_balancer.containers;
    let create_container_result = create_container_instance(&mongo_load_balancer.mongo_image_reference, &load_balancer_id, current_containers).await.unwrap();
    
    let mut load_balancer_containers_mutex = load_balancer.containers.lock().await;
    load_balancer_containers_mutex.push(create_container_result.container_id.clone());
    return Some(create_container_result);
}

pub async fn remove_container_instance (load_balancer_key:&String, docker_container_id:&String)->Vec<String>{
    
    let load_balancers_mutex = LOAD_BALANCERS.get().unwrap().lock().await;
    let load_balancer = load_balancers_mutex.get(load_balancer_key).unwrap();
    let load_balancer_id = load_balancer.id.clone();
    
    let load_balancer_ref = DBCollection::LOADBALANCERS.collection::<docker_models::LoadBalancer>().await.find_one(doc!{
    "_id" : ObjectId::from_str(&load_balancer_id.as_str()).unwrap()
    }, None).await.unwrap().unwrap();
    let mut containers: Vec<String> = load_balancer_ref.containers;
    if let Some(index) = containers.iter().position(|i_container| i_container == docker_container_id){
        containers.remove(index);
    }
    println!("[PROCESS] Removing container:{} from the loadbalancer:{}",docker_container_id, load_balancer_id);
    let _load_balancer_update = DBCollection::LOADBALANCERS.collection::<docker_models::LoadBalancer>().await.find_one_and_update(doc!{
        "_id": ObjectId::from_str(&load_balancer_id.as_str()).unwrap()
    }, doc!{
        "$set" : {
            "containers" : containers.clone()
        }
    }, None).await.unwrap().unwrap();
    println!("[PROCESS] Removing container:{} from the collection", docker_container_id);
    let _container_update = DBCollection::CONTAINERS.collection::<docker_models::Container>().await.find_one_and_delete(doc!{
        "container_id": &docker_container_id
    }, None).await;

    println!("[PROCESS] Database update on load balancer container removal");
    
    return containers;
}
pub async fn check_if_docker_image_exist(docker_image: &String) -> bool{
    
    let docker: &Docker = DOCKER_CONNECTION.get().unwrap();
    let options = Some (ListImagesOptions::<String>{
        all:true,
        ..Default::default()
    });
    let docker_images_result = docker.list_images(options).await.unwrap();
    docker_images_result.iter().filter(|image_summary| image_summary.id.ends_with(docker_image)).collect::<Vec<&ImageSummary>>().len() > 0

}
///fetches the container id
pub async fn route_container(load_balancer_string:String) 
-> (String, usize) 
{
    ActiveServiceDirectory::validate_load_balancer_containers(load_balancer_string.clone()).await;
    ActiveServiceDirectory::next_container(load_balancer_string.clone()).await
    
}

///docker_container_id is based on docker_container_instance and not from the mongodb_container_id
pub async fn try_start_container(docker_container_id:&String)->Result<(),String>{
    let docker = DOCKER_CONNECTION.get().unwrap();
    //check if it is running
    let container_summary_result = docker.inspect_container(&docker_container_id, None).await;
    match container_summary_result{
        Ok(container_summary)=>{
            match container_summary.state.unwrap().status.unwrap() {
        
                ContainerStateStatusEnum::RUNNING => {Ok(())},
                ContainerStateStatusEnum::CREATED => {
                    let start_docker_result = docker.start_container(&docker_container_id, None::<StartContainerOptions<String>>).await;
                    match  start_docker_result{
                        Ok(_)=>{ Ok(())},
                        Err(_) => {Err(format!("Cannot start container {}",docker_container_id))}
                    }
                },
                ContainerStateStatusEnum::EXITED => {
                    let start_docker_result = docker.start_container(&docker_container_id, None::<StartContainerOptions<String>>).await;
                    match  start_docker_result{
                        Ok(_)=>{ Ok(())},
                        Err(_) => {Err(format!("Cannot start container {}",docker_container_id))}
                    }
                },
        
                _ => {
                    Err(format!("Unhandled state condition for {}",docker_container_id))
                }
            }
        },
        Err(_)=>{
            
            Err(format!("cannot inspect container of id:{}", docker_container_id))
        }
    }
    
}
///verifies docker containers if they exist and returns a new vector of the new container id list
pub async fn verify_docker_containers(docker_containers:Vec<String>) -> Vec<String> {
    
    let docker = DOCKER_CONNECTION.get().unwrap();
    let mut container_options_filter = HashMap::new();
    container_options_filter.insert("id".to_string(), docker_containers.clone());
    let list_container_options = ListContainersOptions{
        all: true,
        filters: container_options_filter,
        ..Default::default()
    };
    let result = docker.list_containers(Some(list_container_options)).await.unwrap();
    let new_container_list = result.iter().map(|container_summary| {
        let container_id = <Option<String> as Clone>::clone(&container_summary.id).unwrap().to_string();
        container_id
    }).collect::<Vec<String>>();
    ActiveServiceDirectory::create_container_instances(&new_container_list).await;
    return new_container_list;
}
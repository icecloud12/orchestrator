use std::{collections::HashMap, str::FromStr, sync::OnceLock };
use bollard::{container::{self, Config, CreateContainerOptions, ListContainersOptions, StartContainerOptions}, image::ListImagesOptions, secret::{ContainerStateStatusEnum, HostConfig, ImageSummary, PortBinding}, Docker};
use mongodb::bson::{doc, oid::ObjectId};
use rand::Rng;

use crate::models::{docker_models::{self, Container, ContainerInsert, ContainerRoute, LoadBalancer, LoadBalancerInsert}, load_balancer_models::{ self, ActiveServiceDirectory, LOAD_BALANCERS}};

use super::mongodb_utils::{DBCollection, DATABASE};
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
pub async fn get_load_balancer_instances(docker_image:String, container_address:String) -> String{
    //check local records

    match ActiveServiceDirectory::get_load_balancer_key(container_address.clone()).await {
        Some(index)=>{
            return index;
        },
        None =>{
            //perform a database lookup for a load_balancer
            let load_balancer_result = DBCollection::LOAD_BALANCERS.collection::<LoadBalancer>().await.find_one(doc!{"image" : docker_image.clone()}, None).await.unwrap();
            match load_balancer_result {
                Some(load_balancer) => {
                    //there is an instance of the load balancer and shape of past instance
                    let index = ActiveServiceDirectory::create_load_balancer(load_balancer._id.to_hex(), container_address, LoadBalancerBehavior::RoundRobin, load_balancer.containers).await;
                    return index;
                },
                None => {
                    return  create_load_balancer_instance(docker_image).await
                }
            }
        }
    }
    
    

    
}
///returns load_balancer_key : [type String]
pub async fn create_load_balancer_instance(docker_image:String) -> String{
   
    let doc: LoadBalancerInsert = LoadBalancerInsert{
        image: docker_image.clone(),
        head: 0,
        behavior: LoadBalancerBehavior::RoundRobin.to_string(),
        containers: vec![],
    };
    let create_result: mongodb::results::InsertOneResult = DBCollection::LOAD_BALANCERS.collection::<LoadBalancerInsert>().await.insert_one(doc, None).await.unwrap();

    let find_result = DBCollection::LOAD_BALANCERS.collection::<LoadBalancer>().await.find_one(doc!{ "_id" : create_result.inserted_id}, None).await.unwrap().unwrap();
    
    let container_route = DBCollection::ROUTES.collection::<ContainerRoute>().await.find_one(doc!{"image_name":docker_image }, None).await.unwrap().unwrap();
    
    ActiveServiceDirectory::create_load_balancer(find_result._id.clone().to_hex(), container_route.address, LoadBalancerBehavior::RoundRobin, vec![]).await
    
}

/// returns [type Option]<([type String],[type usize])> as (docker_container_id, container_public_port)
pub async fn create_container_instance (docker_image:&String, load_balancer_id:&String, mut current_containers:Vec<String>)
    -> Option<load_balancer_models::Container>{
  
    println!("creating container instance");
    let docker_image_exist = check_if_docker_image_exist(&docker_image).await;
    println!("docker image exist?:{}",docker_image_exist);
    if  docker_image_exist{
        
        let image_find_result = DBCollection::ROUTES.collection::<ContainerRoute>().await.find_one(doc! {"image_name" : &docker_image}, None).await.unwrap().unwrap();
        let container_port = image_find_result.exposed_port;

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
        let create_container_result = docker.create_container( options, config).await.unwrap();
        let doc = ContainerInsert { 
            image: docker_image.clone(), 
            container_id: create_container_result.id.clone(), 
            public_port: local_port.clone()
        };
        current_containers.push(create_container_result.id.clone());
        println!("load_balancer id fiter {:#?}", &load_balancer_id);
        println!("load_balancer container update {:#?}", &current_containers);
        let _load_balancer_update_result = DBCollection::LOAD_BALANCERS.collection::<docker_models::LoadBalancer>().await.find_one_and_update(
            doc!{"_id": ObjectId::from_str(&load_balancer_id.as_str()).unwrap()},
            doc!{
            "containers": current_containers
        }, None).await;
        let container_insert_result = DBCollection::CONTAINERS.collection::<ContainerInsert>().await.insert_one(doc, None).await.unwrap();
        
        let container = load_balancer_models::Container{
            id: container_insert_result.inserted_id.as_object_id().unwrap().to_hex(),
            container_id: create_container_result.id.clone(),
            public_port: local_port,
            last_accepted_request: None,
            last_replied_request: None,
        };
        println!("created_container model");
        //cannot unlock using mutex_code here
        //update the loadbalancer container field using the mutex data
        // let load_balancers_mutex = LOAD_BALANCERS.get().unwrap().lock().await;
        // println!("load_balancer_mutex");
        // let load_balancer = &load_balancers_mutex[load_balancer_index];
        // let containers_mutex = load_balancer.containers.lock().await;
        // println!("containers mutex");
        // let mut containers = containers_mutex.clone();
        // //and insert the new ID and save
        // containers.push(create_container_result.id.clone());
        // println!("load_balancer_id before update:{:#?}",&load_balancer_id);
        // println!("containers before update :{:#?}",&containers);
        // let _load_balancer_update_result = DBCollection::LOAD_BALANCERS.collection::<LoadBalancer>().await.find_one_and_update(doc!{
        //     "_id" : ObjectId::parse_str(load_balancer_id).unwrap()
        // }, doc!{
        //     "$set": {
        //         "containers": containers
        //     }
        // }, None).await;
        return Some(container);
    }else{
        return None
    }
    
}
pub async fn check_if_docker_image_exist(docker_image: &String) -> bool{
    println!("looking for docker image:{}",docker_image);
    let docker: &Docker = DOCKER_CONNECTION.get().unwrap();
    let options = Some (ListImagesOptions::<String>{
        all:true,
        ..Default::default()
    });
    let docker_images_result = docker.list_images(options).await.unwrap();
    println!("docker image result:{:#?}",docker_images_result);
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
    let container_summary = docker.inspect_container(&docker_container_id, None).await.unwrap();
    
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
    return new_container_list;
}
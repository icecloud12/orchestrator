use std::{borrow::Borrow, cell::OnceCell, collections::HashMap, default, ops::{Deref, DerefMut}, str::FromStr, sync::{Arc, OnceLock, RwLock}};

use axum::{extract::{Host, Request}, response::IntoResponse, routing::Route};
use bollard::{container::{self, Config, CreateContainerOptions, ListContainersOptions, StartContainerOptions}, image::ListImagesOptions, secret::{ContainerStateStatusEnum, ContainerSummary, HostConfig, ImageSummary, Port, PortBinding}, Docker};
use hyper::{StatusCode};
use mongodb::{bson::{doc, oid::ObjectId}, Database};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::models::docker_models::{Container, ContainerInsert, ContainerRoute, LoadBalancer, LoadBalancerInsert};

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


pub async fn get_load_balancer_instances(docker_image:String) -> LoadBalancer{
    let database = DATABASE.get().unwrap();
    let load_balancer_result = database.collection::<LoadBalancer>(DBCollection::LOAD_BALANCERS.to_string().as_str()).find_one(doc!{"image" : docker_image.clone()}, None).await.unwrap();
    match load_balancer_result {
        Some(load_balancer) => {
            //there is an instance of the load balancer
            load_balancer
        },
        None => {
            create_load_balancer_instance(docker_image).await
        }
    }
}

pub async fn create_load_balancer_instance(docker_image:String) -> LoadBalancer{
    let database = DATABASE.get().unwrap();
    let doc: LoadBalancerInsert = LoadBalancerInsert{
        image: docker_image,
        head: 0,
        behavior: LoadBalancerBehavior::RoundRobin.to_string(),
        containers: vec![],
    };
    let create_result: mongodb::results::InsertOneResult = database.collection::<LoadBalancerInsert>(DBCollection::LOAD_BALANCERS.to_string().as_str()).insert_one(doc, None).await.unwrap();

    let find_result = database.collection::<LoadBalancer>(DBCollection::LOAD_BALANCERS.to_string().as_str()).find_one(doc!{ "_id" : create_result.inserted_id}, None).await.unwrap().unwrap();
    find_result
}
pub async fn route_load_balancer(load_balancer:LoadBalancer)  {
    //check container of the load_balancer
    let containers = load_balancer.containers;
    if containers.len() > 0 {
        //fetch the container instance
        //todo port_forward based on container
        //(StatusCode::OK).into_response()
    }else{
        //get get exposed port based from load_balancer image
        let database = DATABASE.get().unwrap();
        let container_route_result = database.collection::<ContainerRoute>(DBCollection::ROUTES.to_string().as_str()).find_one(doc! {
            "image_name" : load_balancer.image.clone()
        }, None).await.unwrap().unwrap();
        let container_create_result  = create_container_instance(
            load_balancer.image,
        ).await;
        match container_create_result {
            Some((container_id, public_port)) =>{
                //start container
                let docker = DOCKER_CONNECTION.get().unwrap();
                return match docker.start_container(container_id.as_str(), None::<StartContainerOptions<String>>).await {
                    Ok(_)=>{
                        //todo port forward request to the container
                        //(StatusCode::OK).into_response()
                    },
                    Err(_)=>{()}
                }
                
            },
            None => ()
        }
        //create an instance of the contaer
    }
}
/// returns [type Option]<([type String],[type usize])> as (docker_container_id, container_public_port)
pub async fn create_container_instance (docker_image:String,)
    -> Option<(String,usize)>{
  
    println!("creating container instance");
    let docker_image_exist = check_if_docker_image_exist(&docker_image).await;
    println!("docker image exist?:{}",docker_image_exist);
    if  docker_image_exist{
        let database = DATABASE.get().unwrap();
        let collection = database.collection::<ContainerRoute>(DBCollection::ROUTES.to_string().as_str());
        let image_find_result = collection.find_one(doc! {"image_name" : &docker_image}, None).await.unwrap().unwrap();
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
        let _ = DBCollection::CONTAINERS.collection::<ContainerInsert>().await.insert_one(doc, None).await;

        let ret = (create_container_result.id, local_port);
        return Some(ret);
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
pub async fn route_container(mut load_balancer:LoadBalancer) -> (String, usize){
    println!("route_container");
    let database = DATABASE.get().unwrap();

    //verify container instances
    let container_ids = verify_docker_containers(&load_balancer).await;
    println!("containerIds:{:#?}",&container_ids);
    load_balancer.containers = container_ids.clone();
    if load_balancer.containers.len() == 0 {
        let (container_id, public_port) = create_container_instance(load_balancer.image).await.unwrap();
        load_balancer.containers.push(container_id);
        let _ = database.collection::<LoadBalancer>(DBCollection::LOAD_BALANCERS.to_string().as_str()).update_one(
            doc!{"_id": load_balancer._id}, 
            doc!{"$set": doc! { "containers": &container_ids}}, 
            None
        ).await;
    }

    //update the head
    let mut head = (&load_balancer.head + 1) % &load_balancer.containers.len();
    let new_head = head as i64;
    let load_balancer_update_result = database.collection::<LoadBalancer>(DBCollection::LOAD_BALANCERS.to_string().as_str()).update_one(
        doc!{"_id": &load_balancer._id}, 
        doc!{"$set": doc! { "head": new_head}}, 
        None
    ).await;
    let container = &load_balancer.containers;
    let container_id = container[head].clone();
    println!("containerResult:{}", &container_id);
    //get container port
    let container_ref = DBCollection::CONTAINERS.collection::<Container>().await.find_one(doc!{"container_id": container_id.clone() }, None).await.unwrap().unwrap();
    return (container_ref.container_id, container_ref.public_port);
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
pub async fn verify_docker_containers(load_balancer:&LoadBalancer) -> Vec<String> {
    let docker_containers = &load_balancer.containers;
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

    

    if !(docker_containers.len() >= new_container_list.len()) {
        let _ = DBCollection::LOAD_BALANCERS.collection::<LoadBalancer>().await.find_one_and_update(doc!{
            "_id": load_balancer._id 
        }, doc!{
                "$set" : doc!{"containers": &new_container_list}
        }, None).await;
    }
    new_container_list
}
use std::{ops::Deref, sync::{Arc, OnceLock}};

use tokio::sync::Mutex;
use crate::utils::docker_utils::LoadBalancerBehavior;


pub struct LoadBalancer {
    pub id: String, //mongo_db_load_balancer_instance
    pub address: String,
    pub head: usize,
    pub behavior: LoadBalancerBehavior,
    pub containers: Arc<Mutex<Vec<String>>> //docker_container_id_instances
}

pub struct Container {
    pub id: String, //references the mongo_db_id_instance
    pub container_id:String, //references the docker_container_id_instance
    pub public_port: usize,
    pub last_accepted_request: Option<String>,
    pub last_replied_request: Option<String>
}


pub static ACTIVE_SERVICE_DIRECTORY: OnceLock<ActiveServiceDirectory> = OnceLock::new();
pub static LOAD_BALANCERS:OnceLock<Arc<Mutex<Vec<LoadBalancer>>>> = OnceLock::new();
pub static CONTAINERS:OnceLock<Arc<Mutex<Vec<Container>>>> = OnceLock::new();
#[derive(Debug)]
pub struct ActiveServiceDirectory {}

impl ActiveServiceDirectory{
    /// returns index of type [type usize] of the generated load_balancer
    pub async fn create_load_balancer(id:String, address:String, behavior: LoadBalancerBehavior, containers:Vec<String>)-> usize{
        let mutex = LOAD_BALANCERS.get_or_init(|| Arc::new(Mutex::new(Vec::new())));
        
        let new_load_balancer = LoadBalancer{
            id,
            address,
            head:0,
            behavior,
            containers : Arc::new(Mutex::new(containers))
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

    pub async fn create_container_instance(mongodb_container_id:String, docker_container_id:String, public_port: usize) -> usize{
        let containers= CONTAINERS.get_or_init(|| Arc::new(Mutex::new(Vec::new())));
        
        let new_container_instance = Container{
            id: mongodb_container_id,
            container_id: docker_container_id,
            public_port: public_port,
            last_accepted_request: None,
            last_replied_request: None,
        };
        let mut mutex = containers.lock().await;
        mutex.push(new_container_instance);
        mutex.len() - 1
    }
}
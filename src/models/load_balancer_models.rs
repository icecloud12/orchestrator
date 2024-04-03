use std::{ops::Deref, sync::{Arc, OnceLock}};

use tokio::sync::Mutex;
use crate::utils::docker_utils::LoadBalancerBehavior;

pub struct LoadBalancer {
    pub id: String,
    pub address: String,
    pub head: usize,
    pub behavior: LoadBalancerBehavior,
    pub containers: Vec<String> //container_references
}
pub static ACTIVE_SERVICE_DIRECTORY: OnceLock<ActiveServiceDirectory> = OnceLock::new();
pub static LOAD_BALANCERS:OnceLock<Arc<Mutex<Vec<LoadBalancer>>>> = OnceLock::new();

#[derive(Debug)]
pub struct ActiveServiceDirectory {}

impl ActiveServiceDirectory{
    pub async fn create_load_balancer(&self, id:String, behavior: LoadBalancerBehavior, address:String){
        let mutex = LOAD_BALANCERS.get_or_init(|| Arc::new(Mutex::new(Vec::new())));
        let new_load_balancer = LoadBalancer{
            id,
            address,
            head:0,
            behavior,
            containers: vec![]
        };
        let mut guard = mutex.lock().await;
        guard.push(new_load_balancer);
       
        
    }
}
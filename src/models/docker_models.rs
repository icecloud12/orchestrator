use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)] 
pub struct ContainerRoute {
    pub image_name: String,
    pub address: String,
    pub prefix: String,
    pub exposed_port: String, //exposed port portrayed in docker container for quick match
}
#[derive(Deserialize)]
pub struct LoadBalancer {
    pub _id: ObjectId,
    pub image:String,
    pub head: usize,
    pub behavior: String,
    pub containers: Vec<String>
}
/// image - docker image
/// 
/// head - current-head-pointer
/// 
/// behavior - behavior type
/// 
/// containers - container-id collection
#[derive(Serialize)]
pub struct LoadBalancerInsert {
    pub image:String,
    pub head: usize,
    pub behavior: String, //defaults to load_balancer
    pub containers: Vec<String>
}

#[derive(Deserialize)]
pub struct Container {
    pub _id: ObjectId,
    pub container_id:String,
    pub image:String,
    pub public_port:usize,
}

#[derive(Serialize)]
pub struct ContainerInsert {

    pub image:String,
    pub container_id:String,
    pub public_port:usize,
}


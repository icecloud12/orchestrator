use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct Image {
    pub _id:ObjectId,
    pub docker_image_id:String
}
pub struct ImageInsert{
    pub docker_image_id:String
}

#[derive(Clone, Debug, Deserialize, Serialize)] 
pub struct Route {
    pub _id: ObjectId,
    pub mongo_image: ObjectId,
    pub address: String,
    pub exposed_port: String, //exposed port portrayed in docker container for quick match
}

#[derive(Deserialize, Serialize)]
pub struct LoadBalancer {
    pub _id: ObjectId,
    pub mongo_image_reference:ObjectId, //mongo_image_reference
    pub head: usize,
    pub behavior: String,
    pub containers: Vec<String>
}
#[derive(Serialize)]
pub struct LoadBalancerInsert {
    pub mongo_image_reference:ObjectId, //mongo_image_reference
    pub head: usize,
    pub behavior: String,
    pub containers: Vec<String>
}

#[derive(Deserialize)]
pub struct Container {
    pub _id: ObjectId,
    pub container_id:String,
    pub mongo_image_reference:ObjectId,
    pub public_port:usize,
    pub last_request: Option<String>,
    pub last_response: Option<String>,
    pub is_detached:Option<bool>
}
#[derive(Serialize,Deserialize)]
pub struct ContainerInsert {
    pub mongo_image_reference:ObjectId,
    pub container_id:String,
    pub public_port:usize,
}


use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct Image {
    pub _id:ObjectId,
    pub docker_image_id:String
}
#[derive(Serialize)]
pub struct ImageInsert{
    pub docker_image_id:String
}

pub enum RouteTypes {
    STATIC,
    CONTAINER
}

impl ToString for RouteTypes {
    
    fn to_string(&self) -> String {
        match &self {
            &Self::STATIC => "static".to_string(),
            &Self::CONTAINER => "container".to_string()
        } 
    }
}
///exposed port will be used differently depending on route_type
/// 
/// static routes will use it directly as is
/// container routes will use it as a setting for creating containers
#[derive(Serialize)]
pub struct RouteInsert{
    pub mongo_image:Option<ObjectId>,
    pub address: String,
    pub exposed_port: String,
    pub route_type:String,
    pub prefix:Option<String>
}



#[derive(Clone, Debug, Deserialize)] 
pub struct Route {
    pub _id: ObjectId,
    pub mongo_image: Option<ObjectId>,
    pub address: String,
    pub exposed_port: String, //exposed port portrayed in docker container for quick match
    pub prefix:Option<String>
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
    pub time_requested: Option<i64>,
    pub time_responded: Option<i64>,
    pub is_detached:Option<bool>
}
#[derive(Serialize,Deserialize)]
pub struct ContainerInsert {
    pub mongo_image_reference:ObjectId,
    pub container_id:String,
    pub public_port:usize,
}




use std::str::FromStr;

use axum::{response::IntoResponse, Json};
use axum_macros::debug_handler;
use hyper::StatusCode;
use mongodb::bson::{doc, oid::ObjectId};
use serde::Deserialize;

use crate::{models::{docker_models::{self, ImageInsert, RouteInsert}, load_balancer_models::ActiveServiceDirectory}, utils::mongodb_utils::DBCollection};
/// addres:[type String] - the general route the router will try to match it with \n
/// exposed_port:[type String] - the container port it will try listening to. Match it with the docker-file exposed port config 
/// docker_image_id:[type String] - the ID of the created docker-image-file

#[derive(Deserialize)]
pub struct AddRoutePayload {
    address: String,
    exposed_port: String,
    docker_image_id: String
}
//todo use docker objectId of docker_image instead where the image is registered first
#[debug_handler]
pub async fn add_route(Json(payload): Json<AddRoutePayload>) -> impl IntoResponse{
    //insert image to the database
    let image_doc = ImageInsert {
        docker_image_id: payload.docker_image_id,
    };
    match DBCollection::IMAGES.collection::<ImageInsert>().await.insert_one(image_doc, None).await {
        Ok(image_insert_result)=>{
            let mongo_image_id: ObjectId = image_insert_result.inserted_id.as_object_id().unwrap();
            // insert route tot he database
            let route_doc = RouteInsert { 
                mongo_image: mongo_image_id, 
                address: payload.address.clone(), 
                exposed_port: payload.exposed_port};
            
            match DBCollection::ROUTES.collection::<RouteInsert>().await.insert_one(route_doc, None).await {
                Ok(route_insert) =>{
                    return (StatusCode::OK, format!("[SUCCESS] Created route (ref: {})", route_insert.inserted_id)).into_response()
                }
                Err(_)=>{
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("[ERROR] Failed in creating route {}", payload.address)).into_response()
                }
            }
        }
        Err(_)=>{
            let error_string = "[ERROR] Cannot insert image";
            println!("{}",error_string);
            return (StatusCode::INTERNAL_SERVER_ERROR, error_string).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct RemoveRoutePayload {
    route_id: String
}

#[debug_handler]
pub async fn remove_route(Json(payload): Json<RemoveRoutePayload>) -> impl IntoResponse{
    //todo get the reference of the route
    let o_id: ObjectId = ObjectId::from_str(payload.route_id.as_str()).unwrap();
    let route_result = DBCollection::ROUTES.collection::<docker_models::Route>().await.find_one(doc!{
        "_id": o_id
    }, None).await.unwrap();

    match route_result {
        Some(route)=>{
            
            
            //todo remove it in database
            match DBCollection::ROUTES.collection::<RemoveRoutePayload>().await.delete_one(doc!{
                "_id": &route._id
            }, None).await {
                Ok(_res)=>{
                    println!("[PROCESS] Successfully deleted route {} from db", &o_id);
                    //todo remove it from the load_balancer_refs
                    
                    if ActiveServiceDirectory::remove_load_balancer(&route.address).await.is_some(){
                        println!("[PROCESS] Successfully removed {} from the router",route.address);
                    }
                }
                Err(e)=>{
                    println!("[PROCESS] Cannot find {} to delete", &o_id);
                }
            };
            return (StatusCode::OK, "").into_response()

        },
        None =>{
            return (StatusCode::OK, "[PROCESS] Cannot find route").into_response()
        }
    }

    

    //remove in    
}
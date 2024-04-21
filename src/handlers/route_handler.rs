

use std::str::FromStr;

use axum::{extract::Path, response::IntoResponse, Json};
use axum_macros::debug_handler;
use hyper::StatusCode;
use mongodb::bson::{doc, oid::ObjectId};
use serde::Deserialize;

use crate::{models::{docker_models::{self, RouteInsert, RouteTypes}, load_balancer_models::ActiveServiceDirectory}, utils::{docker_utils, mongodb_utils::DBCollection}};
/// addres:[type String] - the general route the router will try to match it with \n
/// exposed_port:[type String] - the container port it will try listening to. Match it with the docker-file exposed port config 
/// docker_image_id:[type String] - the ID of the created docker-image-file

#[derive(Deserialize)]
pub struct AddRoutePayload {
    address: String,
    exposed_port: String,
    docker_image_id: Option<String>,
    route_type: String,
    prefix: Option<String>
}
#[debug_handler]
pub async fn add_route(Json(payload): Json<AddRoutePayload>) -> impl IntoResponse{
    
    if payload.route_type == RouteTypes::CONTAINER.to_string() {
        let register_result = docker_utils::register_docker_image(&payload.docker_image_id.unwrap()).await;
        match register_result {
            Ok(registered_image)=>{
                let route_doc = RouteInsert { 
                    mongo_image: Some(registered_image), 
                    address: payload.address.clone(), 
                    exposed_port: payload.exposed_port,
                    route_type: payload.route_type,
                    prefix:payload.prefix,
                };
                match DBCollection::ROUTES.collection::<RouteInsert>().await.insert_one(route_doc, None).await {
                    Ok(route_insert) =>{
                        return (StatusCode::OK, format!("[SUCCESS] Created route (ref: {})", route_insert.inserted_id)).into_response();
                    }
                    Err(_)=>{
                        return (StatusCode::INTERNAL_SERVER_ERROR, format!("[ERROR] Failed in creating route {}", payload.address)).into_response();
                    }
                }
            }
            Err(err)=>{
                return err.into_response()
            }
        };
    }
    else if payload.route_type == RouteTypes::STATIC.to_string(){ 
        let route_doc = RouteInsert { 
            mongo_image: None, 
            address: payload.address.clone(), 
            exposed_port: payload.exposed_port,
            route_type: payload.route_type,
            prefix: payload.prefix
        };
        match DBCollection::ROUTES.collection::<RouteInsert>().await.insert_one(route_doc, None).await {
            Ok(route_insert) =>{
                return (StatusCode::OK, format!("[SUCCESS] Created route (ref: {})", route_insert.inserted_id)).into_response()
            }
            Err(_)=>{
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("[ERROR] Failed in creating route {}", payload.address)).into_response()
            }
        }
    }else{
        return (StatusCode::BAD_REQUEST, format!("[ERROR] Bad Request")).into_response();
    }
    
}


#[debug_handler]
pub async fn remove_route(Path(route_id): Path<String>) -> impl IntoResponse{

    let o_id: ObjectId = ObjectId::from_str(route_id.as_str()).unwrap();
    let route_result = DBCollection::ROUTES.collection::<docker_models::Route>().await.find_one(doc!{
        "_id": o_id
    }, None).await.unwrap();

    match route_result {
        Some(route)=>{
            
            match DBCollection::ROUTES.collection::<docker_models::Route>().await.delete_one(doc!{
                "_id": &route._id
            }, None).await {
                Ok(_res)=>{
                    println!("[PROCESS] Successfully deleted route {} from db", &o_id);
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
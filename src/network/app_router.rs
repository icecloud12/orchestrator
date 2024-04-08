
use std::{time::UNIX_EPOCH};

use axum::{body::{self, Body, to_bytes}, extract::Request, response::IntoResponse, routing::{delete, get, patch, post, put}, Router};
use hyper::{ Method, StatusCode};
use mongodb::{bson::doc, Database};

use crate::{models::docker_models::{Container, LoadBalancer}, utils::{docker_utils::{get_load_balancer_instances, route_container, try_start_container}, mongodb_utils::{DBCollection, DATABASE}}};
use crate::models::docker_models::{ContainerRoute};


pub async fn router()->axum::Router {
    
    let router = Router::new()
        .route("/*path",
        get(active_service_discovery)
        .patch(active_service_discovery)
        .post(active_service_discovery)
        .put(active_service_discovery)
        .delete(active_service_discovery)
        );
        
    return router;
}

pub async fn active_service_discovery(request: Request<Body>) 
-> impl IntoResponse
{   
    let uri = request.uri();
    let response = match route_identifier( &uri.path_and_query().unwrap().to_string()).await {
        Some((docker_image_id, container_path)) => {
            
            //check instances of the load_balancer
            let load_balancer_key =get_load_balancer_instances(docker_image_id.clone(), container_path).await;
            let port_forward_result = port_forward_request(load_balancer_key, request).await;
            port_forward_result.into_response()
        },
        None => {
            (StatusCode::NOT_FOUND).into_response()
        }
    };
    return response;
}




///returns the [type Option]<docker_image_id:[type String], container_path:[type String]>
pub async fn route_identifier(uri:&String) -> Option<(String ,String)>{
    
    let database: &Database = DATABASE.get().unwrap();
    let collection_name = DBCollection::ROUTES.to_string();
    let collection = database.collection::<ContainerRoute>(collection_name.as_str());
    let mut cursor: mongodb::Cursor<ContainerRoute> = collection.find( 
        doc! {
            "$expr": {
                "$eq": [
                    {
                        "$indexOfBytes": [
                            uri.clone(),
                            "$address"
                           
                        ]
                    },
                    0
                ]
            }
          }, None).await.unwrap();
    
    let mut container_route_matches: Vec<ContainerRoute> = Vec::new();
    while cursor.advance().await.unwrap() {
        let document_item: Result<ContainerRoute, mongodb::error::Error> = cursor.deserialize_current();
        match document_item {
            Ok(document) => {
                container_route_matches.push(document);
            }
            Err(_) =>{}
        }        
    }
    if container_route_matches.len() == 0 { //no matching routes
        return None
    }else if container_route_matches.len() == 1 {
        return Some(
            (container_route_matches[0].image_name.clone(),
            container_route_matches[0].address.clone())
        );
    }
    else{
        return Some(route_resolver(container_route_matches, &uri))
    }
        
}
///helper function to help resolve multiple route results
pub fn route_resolver(container_route_matches:Vec<ContainerRoute>, uri:&String) -> (String,String){

    let routes:Vec<Vec<String>> = container_route_matches.iter().map(|container_route| {
        let route:Vec<String> = container_route.address.split("/").filter(|s| s.to_owned()!="").map(String::from).collect();
        route
    }).collect();
    //need to optimize/gets running per split instead of generally at the end
    let uri_split:Vec<String> = uri.split("/").filter(|x| x.to_owned() != "").map(|x| {
        let split_strings = vec!["?", "#"];
        let mut clone_string = x.to_owned();
        for split_string in split_strings{
            clone_string = clone_string.split(split_string).into_iter().collect::<Vec<&str>>()[0].to_string();
        }
        let ret_string: String = clone_string.clone();
        return ret_string
    }).into_iter().collect();

    
    let mut matched_index:usize = 0;
    let mut max_matches:usize = 0;
    for (container_index, container_route) in routes.iter().enumerate() {
        //let mut current_matches:usize = 0;
        let minimun_matches:usize = container_route.len();
        if uri_split.starts_with(container_route) && minimun_matches > max_matches{
            matched_index = container_index;
            max_matches = minimun_matches
        }
    }
    return (
        container_route_matches[matched_index].image_name.clone(),
        container_route_matches[matched_index].address.clone()
    );
}

pub async fn port_forward_request(load_balancer_key:String, request:Request) -> impl IntoResponse{

    let (docker_container_id, public_port) = route_container(load_balancer_key).await; //literal container id
    //try to start the container if not starting
    let forward_request_result = match try_start_container(&docker_container_id).await {
        Ok(_)=>{
            println!("started container");
            //let _ = handshake_and_send(parts, body, container_result.public_port).await;
            let forward_result = forward_request(request, &public_port).await.into_response();
            forward_result
        },
        Err(err)=>{
            //cannot start container
            println!("CANNOT START CONTAINER");
            let res = (StatusCode::INTERNAL_SERVER_ERROR,err).into_response();
            res
        }
    };
    forward_request_result
}

pub async fn forward_request(request:Request, public_port:&usize)
-> impl IntoResponse
{
    
    let (parts, body) = request.into_parts();
    let time = std::time::SystemTime::now();
    let current_time = time.duration_since(UNIX_EPOCH).unwrap().as_secs();
    let mut attempt_time = time.duration_since(UNIX_EPOCH).unwrap().as_secs();
    let maximum_time_attempt_in_seconds:u64 = 3 * 1000;
    println!("forward request");
    let client_builder = reqwest::ClientBuilder::new();
    let client = client_builder.use_rustls_tls().danger_accept_invalid_certs(true).build().unwrap();
    let bytes = to_bytes(body, usize::MAX).await.unwrap();

    let uri = parts.uri;
    let url = format!("https://localhost:{}{}",public_port,uri);
    println!("headers:{:#?}",parts.headers);
    let headers = parts.headers;
    loop { //try to connect till it becomes OK
        attempt_time = time.duration_since(UNIX_EPOCH).unwrap().as_secs();
        if attempt_time - current_time < maximum_time_attempt_in_seconds {
            let method_result = match parts.method {
                Method::GET => {Ok(client.get(&url).headers(headers.clone()).send().await)},
                Method::DELETE => {Ok(client.delete(&url).headers(headers.clone()).body(bytes.clone()).send().await)},
                Method::PATCH => {Ok(client.patch(&url).headers(headers.clone()).body(bytes.clone()).send().await)},
                Method::POST => {Ok(client.post(&url).headers(headers.clone()).body(bytes.clone()).send().await)},
                Method::PUT => {Ok(client.post(&url).headers(headers.clone()).body(bytes.clone()).send().await)}
                _ => {
                    //unhandled method. what to return?
                    Err((StatusCode::INTERNAL_SERVER_ERROR).into_response())
                }
            };
            match method_result {
                Ok(mr_ok) => {
                    let mr_ok_res = match mr_ok {
                        Ok(result) => {
                            let status = &result.status();
                            let res = result.text_with_charset("utf-8").await;
                            return match res {
                                Ok(res_body) => (*status,res_body).into_response(),
                                Err(res_error) => (*status, res_error.to_string()).into_response()
                            };
                        }
                        Err(error) => {
                            if error.status().is_some(){
                                (error.status().unwrap(), error.to_string()).into_response()
                            }else{
                                (StatusCode::INTERNAL_SERVER_ERROR).into_response()
                            }
                        }
                    };
                },
                Err(mr_err)=>{
                    println!("mr:err{:#?}",mr_err)
                }
            };
        }else{
            return (StatusCode::REQUEST_TIMEOUT).into_response()
        }  
    }
}

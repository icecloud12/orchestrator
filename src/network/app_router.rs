
use std::{time::UNIX_EPOCH};

use axum::{body::{to_bytes, Body}, extract::Request, response::IntoResponse, routing::{delete, get, patch, post, put}, Json, Router};
use hyper::StatusCode;
use mongodb::{bson::{doc, oid::ObjectId}, Database};

use crate::{models::{docker_models::Image, load_balancer_models::ActiveServiceDirectory}, utils::{docker_utils::{get_load_balancer_instances, route_container, set_container_latest_reply, set_container_latest_request, try_start_container}, mongodb_utils::{DBCollection, DATABASE}}};
use crate::models::docker_models::Route;
use crate::handlers::route_handler::add_route;

pub async fn router()->axum::Router {
    let prefix = "/asd";

    let router = Router::new()
        .route(format!("{prefix}/add/v1/routes", prefix = prefix).as_str(),post(add_route))
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
    println!("[PROCESS] request:{:#?}",request);
    let uri = request.uri();
    let response = match route_identifier( &uri.path_and_query().unwrap().to_string()).await {
        Some((mongo_image_id, _docker_image_id, container_path)) => {
            
            //check instances of the load_balancer
            let load_balancer_key =get_load_balancer_instances(mongo_image_id.clone(), container_path).await;
            let port_forward_result = port_forward_request(load_balancer_key, request).await;
            port_forward_result.into_response()
        },
        None => {
            (StatusCode::NOT_FOUND).into_response()
        }
    };
    return response;
}
pub async fn test(request:Request<Body>)->impl IntoResponse{
    return (StatusCode::OK).into_response();
}


///returns the [type Option]<mongo_image_id:[type ObjectId], docker_image_id:[type String], container_path:[type String]>
pub async fn route_identifier(uri:&String) -> Option<(ObjectId, String ,String)>{
    println!("[PROCESS] Searching for routes for:{}", &uri);
    let database: &Database = DATABASE.get().unwrap();
    let collection_name = DBCollection::ROUTES.to_string();
    let collection = database.collection::<Route>(collection_name.as_str());
    let mut cursor: mongodb::Cursor<Route> = collection.find( 
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
    
    let mut container_route_matches: Vec<Route> = Vec::new();
    while cursor.advance().await.unwrap() {
        let document_item: Result<Route, mongodb::error::Error> = cursor.deserialize_current();
        println!("[PROCESS] DOCUMENT ITEM:{:#?}", &document_item);
        
        match document_item {
            Ok(document) => {
                container_route_matches.push(document);
            }
            Err(_) =>{}
        }        
    }
    println!("[PROCESS] route matches:{}", container_route_matches.len());
    if container_route_matches.len() == 0 { //no matching routes
        return None
    }else if container_route_matches.len() == 1 {
        let docker_container_image_result = DBCollection::IMAGES.collection::<Image>().await.find_one(doc!{
            "_id" : &container_route_matches[0].mongo_image
        }, None).await.unwrap();
        //(mongo_image_id, docker_image_id, container_path) 
        return Some(
            (   
                container_route_matches[0].mongo_image,
                docker_container_image_result.unwrap().docker_image_id, //#unwrapping error here
                container_route_matches[0].address.clone()
            )
        );
    }
    else{
        return Some(route_resolver(container_route_matches, &uri).await)
    }
        
}
///returns the <mongo_image_id:[type String], docker_image_id:[type String], container_path:[type String]>
pub async fn route_resolver(container_route_matches:Vec<Route>, uri:&String) -> (ObjectId,String,String){

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
    
    let docker_image_result = DBCollection::IMAGES.collection::<Image>().await.find_one(doc!{
        "_id" : container_route_matches[matched_index].mongo_image
    }, None).await.unwrap().unwrap();
    //mongo image, docker_image,path
    return (
        container_route_matches[matched_index]._id.clone(),
        docker_image_result.docker_image_id,
        container_route_matches[matched_index].address.clone()
    );
}

pub async fn port_forward_request(load_balancer_key:String, request:Request) -> impl IntoResponse{

    let (docker_container_id, public_port) = route_container(load_balancer_key.clone()).await; //literal container id
    //create an id for the request
    let request_id:String = ObjectId::new().to_hex();
    //try to start the container if not starting
    let forward_request_result = match try_start_container(&docker_container_id).await {
        Ok(_)=>{
            println!("[PROCESS] Started container {}", &docker_container_id);
            let _ = set_container_latest_request(&docker_container_id, &request_id).await;
            let forward_result = forward_request(request, &public_port).await.into_response();
            let _ = set_container_latest_reply(&docker_container_id, &request_id).await;
            forward_result.into_response()
        },
        Err(_)=>{
            //cannot start container
            println!("[ERROR] Unable to start container: {}", &load_balancer_key);
            match ActiveServiceDirectory::start_container_error_correction(&docker_container_id, &load_balancer_key).await {
                Ok((container_id, public_port))=>{
                    let _ = set_container_latest_request(&container_id, &request_id).await;

                    let forward_result = forward_request(request, &public_port).await.into_response();

                    let _  = set_container_latest_reply(&container_id, &request_id).await;
                    forward_result
                },
                Err(err_response)=>{
                    ActiveServiceDirectory::update_load_balancer_validation(load_balancer_key,false).await;
                    err_response.into_response()
                }
            }
        }
    };
    forward_request_result
}

pub async fn forward_request(request:Request, public_port:&usize)
-> impl IntoResponse
{
    
    let (parts, body) = request.into_parts();
    let time = std::time::SystemTime::now();
    
    let maximum_time_attempt_in_seconds:u64 = 3 * 1000;
    println!("[PROCESS] forwarding request...");
    let client_builder = reqwest::ClientBuilder::new();
    let client = client_builder.use_rustls_tls().danger_accept_invalid_certs(true).build().unwrap();
    let bytes = to_bytes(body, usize::MAX).await.unwrap();

    let uri = &parts.uri;
    let url = format!("https://localhost:{}{}",public_port,uri);
    let host = parts.headers.get("host").unwrap().clone();
    let mut headers = parts.headers.clone();
    let current_time = time.duration_since(UNIX_EPOCH).unwrap().as_secs();
    match headers.get("X-Forwarded-For") {
        Some(header_value) => {
            let mut head_value_split = header_value.to_str().unwrap().split(";").into_iter().map(
                |a| {
                    a.to_string().split(" ").map(|b| b.to_string()).collect::<Vec<String>>().join("")
                }
            ).collect::<Vec<String>>();
            head_value_split.push(host.to_str().unwrap().to_string());
            headers.insert("X-Forwarded-For", head_value_split.join("; ").parse().unwrap());
        }
        None => {
            headers.insert("X-Forwarded-For", host.clone());
        }
    }

    if headers.get("X-Real-Ip").is_none(){
        headers.insert("X-Real-Ip", host.clone());
    }
    //
    
    loop { //try to connect till it becomes OK
        let attempt_time = time.duration_since(UNIX_EPOCH).unwrap().as_secs();
        if attempt_time - current_time < maximum_time_attempt_in_seconds {
            let request_result = client.request(parts.method.clone(), &url).headers(headers.clone()).body(bytes.clone()).send().await;
            let _mr_ok_res = match request_result {
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
        }else{
            return (StatusCode::REQUEST_TIMEOUT).into_response()
        }  
    }
}



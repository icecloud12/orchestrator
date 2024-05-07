
use std::{path::PathBuf, time::UNIX_EPOCH};

use axum::{body::{to_bytes, Body}, extract::Request, response::IntoResponse, routing::{delete, get, patch, post, put}, Json, Router};
use hyper::{HeaderMap, StatusCode, Uri};
use mongodb::{bson::{doc, oid::ObjectId}, Database};

use crate::{handlers::route_handler::remove_route, models::{docker_models::{Image, RouteTypes}, load_balancer_models::ActiveServiceDirectory}, utils::{docker_utils::{get_load_balancer_instances, route_container, set_container_latest_reply, set_container_latest_request, try_start_container}, mongodb_utils::{DBCollection, DATABASE}}};
use crate::models::docker_models::Route;
use crate::handlers::route_handler::add_route;

pub async fn router()->axum::Router {
    let prefix = "/asd";

    let router = Router::new()
        .route(format!("{prefix}/v1/routes/add", prefix = prefix).as_str(),post(add_route))
        .route(format!("{prefix}/v1/routes/remove/:id", prefix = prefix).as_str(), get(remove_route))
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
    println!("[PROCESS] Request: {:#?}", request);
    let uri = request.uri();
    let headers = request.headers();

    let response = match  route_identifier(headers, uri).await {
        Some(T_RouteIdentifierResult) => {
            match T_RouteIdentifierResult {
                RouteIdentifierResult::CONTAINER { mongo_image_id, docker_image_id, container_path , prefix} =>{
                //check instances of the load_balancer
                    let load_balancer_key =get_load_balancer_instances(mongo_image_id.clone(), container_path).await;
                    let port_forward_result = port_forward_request(load_balancer_key, request,prefix, RouteTypes::CONTAINER.to_string() ).await;
                    port_forward_result.into_response()        
                }
                RouteIdentifierResult::STATIC { static_port, prefix } => {
                    let port_forward_result = forward_request(request, static_port ,prefix,RouteTypes::STATIC.to_string()).await;
                    port_forward_result.into_response()
                },
            }
            
        },
        None => {
            return (StatusCode::NOT_FOUND).into_response()
        }
    };
    return response.into_response();
}


pub enum RouteIdentifierResult {
    CONTAINER {mongo_image_id:ObjectId, docker_image_id:String, container_path:String, prefix:Option<String>},
    STATIC {static_port:Option<usize>, prefix:Option<String>}
}

///returns the [type Option]<mongo_image_id:[type ObjectId], docker_image_id:[type String], container_path:[type String]>
pub async fn route_identifier(headers:&HeaderMap, uri: &Uri) -> Option<RouteIdentifierResult>{

    let mut uri_string = String::from("");


    let sec_fetch_site = headers.get("sec-fetch-site");
    //uri_string = uri.path_and_query().clone().unwrap().to_string();

    if sec_fetch_site.is_some() && sec_fetch_site.unwrap().to_str().unwrap() == "same-origin" {
        println!("[PROCESS] Is a referrer");
        let referrer = headers.get("referer").unwrap().to_str().unwrap().to_string();
        let pattern = format!("{}:{}", std::env::var("ADDRESS").unwrap(), std::env::var("PORT").unwrap());
        uri_string = referrer.split(&pattern).into_iter().map(|l| l.to_string()).collect::<Vec<String>>()[1].clone();
        let path_query = uri.path_and_query();
        if path_query.is_some(){
            if(path_query.unwrap().to_string().starts_with(&uri_string)){
                uri_string = path_query.unwrap().to_string()
            }else{
                uri_string = format!("{}{}",uri_string, path_query.unwrap());
            }
        }
        
    }else{
        uri_string = uri.path_and_query().clone().unwrap().to_string();
    }
    
    
    //uri_string = uri.clone();
    println!("[PROCESS] Searching for routes for:{}", &uri_string);
    let database: &Database = DATABASE.get().unwrap();
    let collection_name = DBCollection::ROUTES.to_string();
    let collection = database.collection::<Route>(collection_name.as_str());
    let mut cursor: mongodb::Cursor<Route> = collection.find( 
        doc! {
            "$expr": {
                "$eq": [
                    {
                        "$indexOfBytes": [
                            uri_string.clone(),
                            "$address"
                           
                        ]
                    },
                    0
                ]
            }
          }, None).await.unwrap();
    
    let mut route_matches: Vec<Route> = Vec::new();
    while cursor.advance().await.unwrap() {
        let document_item: Result<Route, mongodb::error::Error> = cursor.deserialize_current();
        
        match document_item {
            Ok(document) => {
                route_matches.push(document);
            }
            Err(_) =>{}
        }        
    }
    println!("[PROCESS] route matches:{}", route_matches.len());
    if route_matches.len() == 0 { //no matching routes
        return None
    }else if route_matches.len() == 1 {
        let current_route = &route_matches[0];
        if current_route.route_type == RouteTypes::CONTAINER.to_string(){
            let docker_container_image_result = DBCollection::IMAGES.collection::<Image>().await.find_one(doc!{
                "_id" : &route_matches[0].mongo_image
            }, None).await.unwrap();
            //(mongo_image_id, docker_image_id, container_path)
            let res = RouteIdentifierResult::CONTAINER{
                mongo_image_id: current_route.mongo_image.unwrap(),
                docker_image_id: docker_container_image_result.unwrap().docker_image_id, //#unwrapping error here
                container_path: current_route.address.clone(),
                prefix: current_route.prefix.clone(),
                
            };
            return Some(res);
        }else {
            return Some (RouteIdentifierResult::STATIC { static_port: Some(route_matches[0].exposed_port.clone().parse::<usize>().unwrap()), prefix: route_matches[0].prefix.clone() });
        }
    }
    else{
        return Some(route_resolver(route_matches, &uri_string).await)
    }
        
}
///returns the <mongo_image_id:[type String], docker_image_id:[type String], container_path:[type String]>
pub async fn route_resolver(route_matches:Vec<Route>, uri:&String) -> RouteIdentifierResult{

    let routes:Vec<Vec<String>> = route_matches.iter().map(|matched_route| {
        let route:Vec<String> = matched_route.address.split("/").filter(|s| s.to_owned()!="").map(String::from).collect();
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
        "_id" : route_matches[matched_index].mongo_image
    }, None).await.unwrap().unwrap();

    if route_matches[matched_index].route_type == RouteTypes::CONTAINER.to_string(){
        return RouteIdentifierResult::CONTAINER { 
            mongo_image_id: route_matches[matched_index].mongo_image.clone().unwrap(),
            docker_image_id: docker_image_result.docker_image_id, 
            container_path: route_matches[matched_index].address.clone(),
            prefix: route_matches[matched_index].prefix.clone()
        }
    }else {
        return RouteIdentifierResult::STATIC { static_port:Some(route_matches[matched_index].exposed_port.clone().parse::<usize>().unwrap()) ,prefix:route_matches[matched_index].prefix.clone() }
    }
    
}

pub async fn port_forward_request(load_balancer_key:String, request:Request, prefix: Option<String>, route_type:String) -> impl IntoResponse{

    let (docker_container_id, public_port) = route_container(load_balancer_key.clone()).await; //literal container id
    //create an id for the request
    let request_id:String = ObjectId::new().to_hex();
    //try to start the container if not starting
    let forward_request_result = match try_start_container(&docker_container_id).await {
        Ok(_)=>{
            println!("[PROCESS] Started container {}", &docker_container_id);
            let _ = set_container_latest_request(&docker_container_id, &request_id).await;
            let forward_result = forward_request(request, Some(public_port) ,prefix, route_type).await.into_response();
            let _ = set_container_latest_reply(&docker_container_id, &request_id).await;
            forward_result.into_response()
        },
        Err(_)=>{
            //cannot start container
            println!("[ERROR] Unable to start container: {}", &load_balancer_key);
            match ActiveServiceDirectory::start_container_error_correction(&docker_container_id, &load_balancer_key).await {
                Ok((container_id, public_port))=>{
                    let _ = set_container_latest_request(&container_id, &request_id).await;

                    let forward_result = forward_request(request, Some(public_port),prefix, route_type).await.into_response();

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

pub async fn forward_request(request:Request, public_port:Option<usize>, prefix: Option<String>, route_type:String)
-> impl IntoResponse
{
    
    let (parts, body) = request.into_parts();
    let time = std::time::SystemTime::now();
    let current_time = time.duration_since(UNIX_EPOCH).unwrap().as_secs();
    let maximum_time_attempt_in_seconds:u64 = 5;
    
    let client_builder = reqwest::ClientBuilder::new();
    let client = client_builder.use_rustls_tls().danger_accept_invalid_certs(true).build().unwrap();
    let bytes = to_bytes(body, usize::MAX).await.unwrap();

    let headers = parts.headers.clone();
    let uri = extract_uri( &parts.uri, prefix);
    //let uri = parts.uri.path_and_query().unwrap().to_string();
    match &route_type {
        static_route if  RouteTypes::STATIC.to_string() == route_type => {
            let url = if public_port.is_some(){
                format!("https://localhost:{}{}",public_port.unwrap(),uri)
            }else{
                format!("https://localhost{}",uri)
            };
            println!("[PROCESS] forwarding request to static {}", &url );
            //forwarding to static route
            let attempt_time = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
            let request_result = client.request(parts.method.clone(), &url).headers(headers.clone()).body(bytes.clone()).send().await;
            let _mr_ok_res = match request_result {
                Ok(result) => {
                    let response_time = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
                    let response_diff = response_time - attempt_time;
                    let status = result.status();
                    //let bytes = result.bytes().await.unwrap();
                    let headers = result.headers().clone();
                    let body = Body::try_from(result.bytes().await.unwrap()).unwrap();
                    let status_code = StatusCode::from_u16(status.as_u16()).unwrap();
                    println!("[PROCESS] Responded");
                    //todo insert to request db
                    return (status_code,headers,body).into_response();
                    
                }
                Err(error) => { //i think this is wrong
                    println!("[PROCESS] Failed to fetch to destination");
                    return (StatusCode::BAD_GATEWAY).into_response();
                }
            };
        },
        container_route if RouteTypes::CONTAINER.to_string() == route_type => {
            let url =  format!("https://localhost:{}{}",public_port.unwrap(),uri);
            println!("[PROCESS] forwarding request to container {}", &url );
            loop { //try to connect till it becomes OK
                let attempt_time = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                if attempt_time - current_time < maximum_time_attempt_in_seconds {
                    println!("[PROCESS] current attempt time: {:#?}/{}", (attempt_time - current_time), maximum_time_attempt_in_seconds);
                    let request_result = client.request(parts.method.clone(), &url).headers(headers.clone()).body(bytes.clone()).send().await;
                    let _mr_ok_res = match request_result {
                        Ok(result) => {
                            let status = result.status();
                            //let bytes = result.bytes().await.unwrap();
                            let headers = result.headers().clone();
                            let body = Body::try_from(result.bytes().await.unwrap()).unwrap();
                            
                            let status_code = StatusCode::from_u16(status.as_u16()).unwrap();
                            println!("[PROCESS] Responded");
                             //todo insert to request db
                            return (status_code,headers,body).into_response();
                            
                        }
                        Err(error) => { //i think this is wrong
                            println!("[PROCESS] Failed: Retrying");
                        }
                    };
                }else{
                    println!("[PROCESS] Responded but failed");
                    return (StatusCode::REQUEST_TIMEOUT).into_response()
                }  
            }
        },
        _ => {
            println!("[ERROR] Unable to handle route type of {}", route_type);
            return (StatusCode::BAD_REQUEST).into_response();  
        }
    };
    
    
}

//
pub fn extract_uri (uri:&Uri, prefix: Option<String>)->String {
    let new_uri = if uri.path_and_query().is_some() {

        let temp = uri.path_and_query().unwrap().to_string();
        if prefix.is_some(){
            let prefix_t: String = prefix.unwrap();
            if temp.starts_with(&format!("/{}", prefix_t)){
                let split = temp.split(&format!("/{}", prefix_t)).into_iter().map(|x| x.to_string()).collect::<Vec<String>>();
                if split.len() == 1 {
                    split[0].clone()
                }else if split.len() == 2{
                    split[1].clone()
                }else{
                    split[0 .. split.len()-1].into_iter().map(|x|x.to_string()).collect::<Vec<String>>().join("")
                }
            }else{
                temp
            }
        }else{
            temp
        }
    }else{
        "/".to_string()
    };
    new_uri
}

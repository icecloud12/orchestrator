
use std::{collections::HashMap, ops::Deref, str::FromStr};

use axum::{body::{self, Body}, extract::{DefaultBodyLimit, Request, State}, response::IntoResponse, routing::{delete, get, patch, post, put}, Router};
use bollard::auth;
use hyper::{upgrade::Upgraded, Method, StatusCode, Uri, Response};
use mongodb::{bson::{bson, doc, oid::ObjectId, Bson}, Database};
use reqwest::Client;
use std::net::SocketAddr;

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::client::conn::http1::Builder;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::tokio::TokioIo;


use tokio::{io::{self, AsyncWriteExt as _}, net::{TcpListener, TcpStream}};

use crate::{models::docker_models::{Container, LoadBalancer}, utils::{docker_utils::{get_load_balancer_instances, route_container, route_load_balancer, try_start_container}, mongodb_utils::{DBCollection, DATABASE}}};
use crate::models::docker_models::{ContainerRoute};


pub async fn router()->axum::Router {
    let client = Client::new();
    let router = Router::new()
        .route("/*path",
            delete(active_service_discovery)
            .get(active_service_discovery)
            .patch(active_service_discovery)
            .post(active_service_discovery)
            .put(active_service_discovery)
        ).with_state(client);
        
    return router;
}

pub async fn active_service_discovery(State(client): State<Client>, request: Request) -> impl IntoResponse{
    
    let uri = request.uri();
    let _method = request.method();
    let _body = request.body();
    let _header = request.headers();

    let response = match route_identifier(uri.path_and_query().unwrap().to_string()).await {
        Some(docker_image) => {
            println!("{}", docker_image);
            //check instances of the load_balancer
            let load_balancer =get_load_balancer_instances(docker_image).await;
            

            port_forward_request(client, load_balancer, request).await;
           (StatusCode::OK).into_response() 
        },
        None => {
            (StatusCode::NOT_FOUND).into_response()
        }
    };
    return response;
}




///returns the Router Docker Image 
pub async fn route_identifier(uri:String) -> Option<String>{
    
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
        return Some(container_route_matches[0].image_name.clone());
    }
    else{
        return Some(route_resolver(container_route_matches, uri))
    }
        
}
///helper function to help resolve multiple route results
pub fn route_resolver(container_route_matches:Vec<ContainerRoute>, uri:String) -> String{

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
    return container_route_matches[matched_index].image_name.clone();
}

pub async fn port_forward_request(client:Client, load_balancer:LoadBalancer, request:Request){
    let database = DATABASE.get().unwrap();
    let http = "https";

    let container_id = route_container(load_balancer).await;
    let object_id:ObjectId = ObjectId::from_str(container_id.as_str()).unwrap();
    println!("{:#?}",object_id);
    let container_result = database.collection::<Container>(DBCollection::CONTAINERS.to_string().as_str()).find_one(doc! {"_id": object_id}, None).await.unwrap().unwrap();

    //try to start the container if not starting
    match try_start_container(container_result.container_id).await {
        Ok(_)=>{let body = request.into_body();
            let _ = handshake(format!("{http_type}://0.0.0.0:{port}", http_type=http, port=container_result.public_port), body).await;},
        Err(_)=>{}
    }

    
}

pub async fn handshake(url:String, body:Body)->Result<(),()>{
    //open a TCP connection to the remote host
    let remote_url =    url.parse::<hyper::Uri>().unwrap();
    let host = remote_url.host().expect("uri has no host");
    let port = remote_url.port_u16().unwrap_or(80);

    let address = format!("{}:{}", host, port);
    match TcpStream::connect(address.clone()).await {
        Ok(stream) =>{
            let io = TokioIo::new(stream);
            let (mut sender, conn) =  hyper::client::conn::http1::handshake(io).await.unwrap();
            tokio::task::spawn(async move {
                if let Err(err) = conn.await {
                    println!("Connection failed: {:?}", err);
                }
            });
            let authority = remote_url.authority().unwrap().clone();
            let req = Request::builder()
            .uri(remote_url)
            .header(hyper::header::HOST, authority.as_str())
            .body(body).unwrap();
            let mut res = sender.send_request(req).await.unwrap();
            // Stream the body, writing each frame to stdout as it arrives
            while let Some(next) = res.frame().await {
                let frame = next.unwrap();
                if let Some(chunk) = frame.data_ref() {
                    io::stdout().write_all(chunk).await.unwrap();
                }
            }  
        },
        Err(_)=>{
            //cannot create a tcp_stream connection
        }
    }
    Ok(())
    
}




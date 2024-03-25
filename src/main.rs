use std::{os::windows::process, sync::{Arc}};

use bollard::Docker;
use dotenv::dotenv;
use network::app_router::router;
use utils::{docker_utils:: DOCKER_CONNECTION, mongodb_utils::DATABASE};
mod utils;
mod network;
mod models;
#[tokio::main]
async fn main() {
    dotenv().ok();
    DOCKER_CONNECTION.get_or_init(|| Docker::connect_with_local_defaults().unwrap());
    match DATABASE.set(utils::mongodb_utils::connect().await) {
        Ok(_)=>{
            listen().await;
        },
        Err(_)=>{
            println!("Cannot connect to database...exiting");
            std::process::exit(0);
        }
    }  
}
async fn listen(){
    let router = router().await;
    let address = std::env::var("ADDRESS").unwrap();
    let port = std::env::var("PORT").unwrap();
    let listener = tokio::net::TcpListener::bind(format!("{address}:{port}",address = &address, port = &port)).await.unwrap();
    println!("now listening in {}:{}",address,port);
    axum::serve(listener, router).await.unwrap(); 
}

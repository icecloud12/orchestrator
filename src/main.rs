use std::os::windows::process;

use dotenv::dotenv;
use network::app_router::router;
use utils::mongodb_utils::Database;
mod utils;
mod network;
#[tokio::main]
async fn main() {
    dotenv().ok();
    //setup database connection
    match Database.set(utils::mongodb_utils::connect().await) {
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

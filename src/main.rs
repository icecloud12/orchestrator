use std::{env, net::SocketAddr, path::PathBuf, process::exit};
use axum_server::{tls_rustls::RustlsConfig};
use bollard::Docker;
use dotenv::dotenv;

use network::app_router;
use utils::{docker_utils:: DOCKER_CONNECTION, mongodb_utils::DATABASE};
mod utils;
mod network;
mod models;
mod handlers;
#[tokio::main]
async fn main() {
    dotenv().ok();

    DOCKER_CONNECTION.get_or_init(|| {
		match Docker::connect_with_local_defaults() {
			Ok(docker_connection) => {
				println!("{:#?}", &docker_connection);
				docker_connection
			},
			Err(error) => {
				println!("{}", error);
				exit(0x0100)
			},
		}
	});
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
#[allow(dead_code)]
#[derive(Clone, Copy)]
struct Ports {
    https: u16,
}
async fn listen(){

    match RustlsConfig::from_pem_file(
    PathBuf::from("./src/keys")
        .join("orchestrator.crt"),
        PathBuf::from("./src/keys")
        .join("orchestrator_pem.pem"),
    ).await {
    Ok(config) => {
        let router = app_router::router().await;
        let ip = env::var("ADDRESS").unwrap().split(".").into_iter().map(|x| x.parse::<u8>().unwrap()).collect::<Vec<u8>>();
        let socket_address = [ip[0],ip[1],ip[2],ip[3]]; 
        // run https server
        let addr = SocketAddr::from((socket_address, env::var("PORT").unwrap().parse::<u16>().unwrap()));
        let addr_s = &addr.to_string();
        println!("listening on {}", addr_s);
        axum_server::bind_rustls(addr, config)
            .serve(router.into_make_service())
            .await
            .unwrap();
        
    },
    Err(e) =>{ println!("error:{:#?}",e)}
   };
   

}

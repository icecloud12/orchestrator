use std::{env, net::SocketAddr, os::windows::process, path::PathBuf, sync::Arc};
use axum::{extract::Host, handler::HandlerWithoutStateExt, response::Redirect, BoxError};
use axum_server::{service::SendService, tls_rustls::RustlsConfig};
use bollard::Docker;
use dotenv::dotenv;
use hyper::{StatusCode, Uri};

use network::app_router::{self, router};
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
#[allow(dead_code)]
#[derive(Clone, Copy)]
struct Ports {
    http: u16,
    https: u16,
}
async fn listen(){
    let ports = Ports {
        http: 3000,
        https: 3001
    };

    tokio::spawn(redirect_http_to_https(ports));
    match RustlsConfig::from_pem_file(
    PathBuf::from(r"C:\nginx\")
        .join("localhost.crt"),
        PathBuf::from(r"C:\nginx\")
        .join("localhost.key"),
    ).await {
    Ok(config) => {
        let router = app_router::router().await;
        let ip = env::var("ADDRESS").unwrap().split(".").into_iter().map(|x| x.parse::<u8>().unwrap()).collect::<Vec<u8>>();
        let socket_address = [ip[0],ip[1],ip[2],ip[3]]; 
        // run https server
        let addr = SocketAddr::from((socket_address, ports.https));
        axum_server::bind_rustls(addr, config)
            .serve(router.into_make_service())
            .await
            .unwrap();
    },
    Err(e) =>{ println!("error:{:#?}",e)}
   };
   

}
#[allow(dead_code)]
async fn redirect_http_to_https(ports: Ports) {
    fn make_https(host: String, uri: Uri, ports: Ports) -> Result<Uri, BoxError> {
        let mut parts = uri.into_parts();

        parts.scheme = Some(axum::http::uri::Scheme::HTTPS);

        if parts.path_and_query.is_none() {
            parts.path_and_query = Some("/".parse().unwrap());
        }

        let https_host = host.replace(&ports.http.to_string(), &ports.https.to_string());
        parts.authority = Some(https_host.parse()?);

        Ok(Uri::from_parts(parts)?)
    }

    let redirect = move |Host(host): Host, uri: Uri| async move {
        match make_https(host, uri, ports) {
            Ok(uri) => Ok(Redirect::permanent(&uri.to_string())),
            Err(error) => {
                //tracing::warn!(%error, "failed to convert URI to HTTPS");
                Err(StatusCode::BAD_REQUEST)
            }
        }
    };

    let ip = env::var("ADDRESS").unwrap().split(".").into_iter().map(|x| x.parse::<u8>().unwrap()).collect::<Vec<u8>>();
    let socket_address = [ip[0],ip[1],ip[2],ip[3]]; 
    // run https server
    let addr = SocketAddr::from((socket_address, ports.http));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, redirect.into_make_service())
        .await
        .unwrap();
}
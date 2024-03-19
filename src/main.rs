use dotenv::dotenv;
use network::app_router::router;
mod network;
#[tokio::main]
async fn main() {
    dotenv().ok();
    let router = router().await;
    let address = "192.168.3.8";
    let port = 3000;
    let listener = tokio::net::TcpListener::bind(format!("{address}:{port}",address = &address, port = &port)).await.unwrap();
    println!("now listening in {}:{}",address,port);
    axum::serve(listener, router).await.unwrap();    
}

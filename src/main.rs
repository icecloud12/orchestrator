use dotenv::dotenv;
use network::app_router::router;
mod network;
#[tokio::main]
async fn main() {
    dotenv().ok();
    let router = router().await;
    let address = std::env::var("ADDRESS").unwrap();
    let port = std::env::var("PORT").unwrap();
    let listener = tokio::net::TcpListener::bind(format!("{address}:{port}",address = &address, port = &port)).await.unwrap();
    println!("now listening in {}:{}",address,port);
    axum::serve(listener, router).await.unwrap();    
}

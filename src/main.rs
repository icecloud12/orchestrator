use dotenv::dotenv;

mod network;
#[tokio::main]
async fn main() {
    dotenv().ok();
}

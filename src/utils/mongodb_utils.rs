use std::sync::OnceLock;
use mongodb::{options::ClientOptions, Client, Database};

pub static Database:OnceLock<Database> = OnceLock::new();

pub async fn connect()-> Database{
    //database connection
    let options = ClientOptions::parse(std::env::var("DATABASE_URI").unwrap()).await.unwrap();
    let client = Client::with_options(options).unwrap();
    let database = client.database(std::env::var("DATABASE_NAME").unwrap().as_str());
    return database;
}

pub enum DBCollection {
    ROUTES,
}

impl ToString for DBCollection {
    fn to_string(&self) -> String {
        match &self {
            &Self::ROUTES => "routes".to_string()
        }    
    }
}
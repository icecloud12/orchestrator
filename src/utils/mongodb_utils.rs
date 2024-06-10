use std::sync::OnceLock;
use mongodb::{options::ClientOptions, Client, Collection, Database};

pub static DATABASE:OnceLock<Database> = OnceLock::new();

pub async fn connect()-> Database{
    //database connection
	
	let options:ClientOptions = ClientOptions::parse(std::env::var("DATABASE_URI").unwrap()).await.unwrap();
	let client = Client::with_options(options).unwrap();
	let database = client.database(std::env::var("DATABASE_NAME").unwrap().as_str());
	return database;
    
}

pub enum DBCollection {
    IMAGES,
    ROUTES,
    LOADBALANCERS,
    CONTAINERS,
}

impl ToString for DBCollection {
    fn to_string(&self) -> String {
        match &self {
            &Self::IMAGES => "images".to_string(),
            &Self::ROUTES => "routes".to_string(),
            &Self::LOADBALANCERS => "load_balancers".to_string(),
            &Self::CONTAINERS => "containers".to_string(),
        }    
    }
}

impl DBCollection {
    pub async fn collection<T>(&self)->Collection<T>{
        match &self {
            &Self::IMAGES => DATABASE.get().unwrap().collection::<T>(DBCollection::IMAGES.to_string().as_str()),
            &Self::ROUTES => DATABASE.get().unwrap().collection::<T>(DBCollection::ROUTES.to_string().as_str()),
            &Self::LOADBALANCERS => DATABASE.get().unwrap().collection::<T>(DBCollection::LOADBALANCERS.to_string().as_str()),
            &Self::CONTAINERS => DATABASE.get().unwrap().collection::<T>(DBCollection::CONTAINERS.to_string().as_str()),
        }
    }
}
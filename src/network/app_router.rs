use std::{collections::HashMap, hash::Hash, process::{self, exit}};

use axum::{extract::{DefaultBodyLimit, Request}, http::uri, routing::{delete, get, patch, post, put}, Router};
use bollard::{container::{self, Config, CreateContainerOptions, ListContainersOptions, StartContainerOptions}, image::ListImagesOptions, secret::{HostConfig, Port, PortBinding}, Docker};
use hyper::Uri;
use mongodb::{bson::doc, options::CreateCollectionOptions, Database};
use serde::{Deserialize, Serialize};
use crate::utils::mongodb_utils::{self, DBCollection, Database};

pub async fn router()->axum::Router {
    let router = Router::new()
        .route("/*path",
            delete(active_service_discovery)
            .get(active_service_discovery)
            .patch(active_service_discovery)
            .post(active_service_discovery)
            .put(active_service_discovery)
        );
        
    return router;
}

pub async fn active_service_discovery(
        //path:Path<Vec<(String, String)>>, 
        //query(Query(params)):Query<HashMap<String,String>>,
        //header:HeaderMap, payload:Option<Json<Value>>)
        request: Request){
    // create logic in listen docker containers
    //println!("{:#?}", &request);
    let uri = request.uri();
    let _method = request.method();
    let _body = request.body();
    let _header = request.headers();

    
    //need to do some logic where uri = docker_route
    let _a = docker_manager(uri.to_string()).await;
    //check the request who the request is for (which server is going to be hit)
    // check if there is an active docker container for it
            //fetch port
    // if not create instance of docker container
            //fetch port
    //port forward to the appropriate container
    //return result of container
}

#[derive(Clone, Debug, Deserialize, Serialize)] 
struct ContainerRoute {
    pub image_name: String,
    pub address: String,
    pub prefix: String,
}

pub async fn docker_manager(docker_uri:String){
    let docker_connect: Result<Docker, bollard::errors::Error> = Docker::connect_with_named_pipe_defaults();
    match docker_connect {
        Ok(docker) =>{
            println!("can connect to docker");
            let database: &Database = mongodb_utils::Database.get().unwrap();
            let container_route_result = database.collection::<ContainerRoute>(DBCollection::ROUTES.to_string().as_str()).find_one(doc! {"address": "/lcr/api/"}, None).await;
            println!("contaier_route_results:{:#?}",container_route_result);
            match container_route_result.unwrap() {
                Some(container_route)=>{
                    println!("some container route:{:#?}",container_route);
                    let image = container_route.image_name;
                    //get containers that instaces this
                    let mut filters = HashMap::new();
                    filters.insert("health", vec!["none"]);
                    filters.insert("ancestor", vec![image.as_str()]);
                    let container_options = Some(ListContainersOptions {
                        all: true,
                        filters: filters,
                        ..Default::default()
                    });
                    let containers_result: Result<Vec<bollard::secret::ContainerSummary>, bollard::errors::Error> = docker.list_containers(container_options).await;
                    //need to implement a load balancer for the containers
                    println!("containers search result{:#?}",containers_result);

                    {
                        let mut filters = HashMap::new();
                        filters.insert("dangling", vec!["false"]);

                        let options = Some(ListImagesOptions{
                        all: true,
                        filters,
                        ..Default::default()
                        });
                        println!("image List");
                        println!("{:#?}",docker.list_images(options).await);

                    }

                    match containers_result {
                        Ok(containers)  =>{
                            
                            if containers.len() > 0{
                                println!("list of contaienrs {:#?}",containers);
                                //get first instance for now
                                let container = containers[0].clone();
                                println!("containerState:{:#?}", container.state)
                            }else{
                                println!("generate container:{}",image);
                                
                                let local_port: u16 = 3001;
                                let container_port:u16 = 4002;
                                let mut port_binding = HashMap::new();
                                port_binding.insert(container_port.to_string(), Some(vec![PortBinding {
                                    //external port
                                    host_port: Some(local_port.to_string()),
                                    //localhost
                                    host_ip: Some("0.0.0.0".to_string())
    
                                }]));
                                let host_config = HostConfig {
                                    
                                    port_bindings: Some(port_binding),
                                    network_mode: Some("bridge".to_string()),
                                    ..Default::default()
                                };
                                let mut exposed_ports = HashMap::new();
                                exposed_ports.insert("4002/tcp", HashMap::new());
                                //static image for now
                                let test_image:String = "72b3512dbf1fd82c4df4f884ac85898bfd11e5b1bc6c491c3f75592583fd22c7".to_string();
                                let container_config = Config {
                                    
                                    image:Some(test_image.as_str()),
                                    exposed_ports:Some(exposed_ports),
                                    host_config: Some(host_config),
                                    ..Default::default()
                                };
                                
                                match  docker.create_container(None::<CreateContainerOptions<&str>>,container_config).await {
                                    Ok(container_create_result) => {
                                        //find container name by id
                                        println!("created_container_result:{:#?}",container_create_result);
                
                                        let mut filters = HashMap::new();
                                        filters.insert("id", vec![container_create_result.id.as_str()]);
                                        let list_container_options = ListContainersOptions {
                                            all: true,
                                            filters: filters,
                                            ..Default::default()
                                        };
                                        let container_search_result = docker.list_containers(Some(list_container_options)).await.unwrap(); //expects 1 result
                                        if container_search_result.get(0).is_some() {
                                            println!("container_search_result:{:#?}",container_search_result);
                                            let container = container_search_result[0].clone();
                                            let container_name = container.id.unwrap();
                                            let start_container_result = docker.start_container(container_name.as_str(), None::<StartContainerOptions<String>>).await;
                                            match start_container_result {
                                                Ok(_) => {
                                                    let port: &Port = &container.ports.unwrap()[0];
                                                    println!("new container running in port:{:#?}", port.private_port);
                                                },
                                                Err(_) => {
                                                    println!("cannot start container")
                                                }
                                            };

                                        }
                                    },
                                    Err(e) => {
                                        println!("Cannot create container:{}", e);
                                    }
                                };

                            }
                        }
                        Err(e) => {
                            println!("Cannot generate list of containers: {}",e)
                        }
                    }
                    

                }
                None => {
                    //transfer to 404 page
                    println!("cannot find containerRoute");
                }
            }
            

        },
        Err(e) =>{
            println!("Cannot connect to docker: {e}", e=e);
            std::process::exit(0)
        }
    }
}
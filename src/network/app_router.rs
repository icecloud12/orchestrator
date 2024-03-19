use axum::{extract::{DefaultBodyLimit, Request}, routing::{delete, get, patch, post, put}, Router};
use hyper::HeaderMap;
use serde_json::Value;

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
    println!("{:#?}", &request);
    let uri = request.uri();
    let method = request.method();
    let body = request.body();
    let header = request.headers();

    //check the request who the request is for (which server is going to be hit)
    // check if there is an active docker container for it
            //fetch port
    // if not create instance of docker container
            //fetch port
    //port forward to the appropriate container
    //return result of container
}
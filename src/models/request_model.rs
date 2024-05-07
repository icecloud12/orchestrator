use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct Request{
    pub _id: ObjectId,
    pub time_sent: u128,
    pub time_responded: u128,
    pub time_diff: u128,
    pub status_code: String
}

#[derive(Serialize)]
pub struct InsertRequest{
    pub _id: ObjectId,
    pub time_sent: u128,
    pub time_responded: u128,
    pub time_diff: u128,
    pub status_code: String
}
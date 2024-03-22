use std::sync::Mutex;

use bollard::secret::ContainerSummary;
pub struct ContainerRoute {
    route:String,
    containers: Mutex<Vec<ContainerSummary>>
}
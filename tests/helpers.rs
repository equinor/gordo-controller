use kube::api::{DeleteParams, ListParams};
use kube::{api::Api, client::APIClient, config};
use serde_json::Value;
use serde_yaml;

use gordo_controller::crd::gordo::Gordo;

// Get the `APIClient` using current kube config
pub async fn client() -> APIClient {
    let config = config::load_kube_config()
        .await
        .unwrap_or_else(|_| config::incluster_config().expect("Failed to get local kube config and incluster config"));
    APIClient::new(config)
}

// Remove _all_ gordos.
pub async fn remove_gordos(gordos: &Api<Gordo>) {
    for gordo in gordos.list(&ListParams::default()).await.unwrap().items.iter() {
        gordos
            .delete(&gordo.metadata.name, &DeleteParams::default())
            .await
            .unwrap();
    }
}

// Get the repo's example `Gordo` config file
pub fn example_config(name: &str) -> Value {
    let config_str = std::fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), name))
        .expect("Failed to read config file");
    serde_yaml::from_str(&config_str).expect("Unable to parse config file into yaml")
}

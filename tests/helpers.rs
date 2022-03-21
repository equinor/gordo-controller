#![allow(dead_code)]
use kube::api::{DeleteParams, ListParams};
use kube::{api::Api, client::Client};
use serde_yaml;

use gordo_controller::crd::gordo::Gordo;
use serde::de::DeserializeOwned;

// Get the `APIClient` using current kube config
pub async fn client() -> Client {
    Client::try_default().await.expect("Unable to create default Client")
}

// Remove _all_ gordos.
pub async fn remove_gordos(gordos: &Api<Gordo>) {
    for gordo in gordos.list(&ListParams::default()).await.unwrap().items.iter() {
        let name = gordo.metadata.name.clone().expect("gordo.metadata.name is empty");
        gordos
            .delete(&name, &DeleteParams::default())
            .await
            .unwrap();
    }
}

// Get the repo's example `Gordo` config file
pub fn deserialize_config<T>(name: &str) -> T
    where T: DeserializeOwned 
{
    let config_str = std::fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), name))
        .expect("Failed to read config file");
    serde_yaml::from_str(&config_str).expect("Unable to parse config file into yaml")
}

/// Load the `Gordo` custom resource API interface
pub fn load_gordo_resource(client: Client, namespace: &str) -> Api<Gordo> {
    Api::namespaced(client.clone(), namespace)
}

use kube::api::{DeleteParams, ListParams};
use kube::{api::Api, client::APIClient, config};
use serde_json::Value;
use serde_yaml;

use crate::Gordo;

// Get the `APIClient` using current kube config
pub async fn client() -> APIClient {
    let config = config::load_kube_config().await.unwrap_or_else(|_| {
        config::incluster_config().expect("Failed to get local kube config and incluster config")
    });
    APIClient::new(config)
}

// Get an API to the Gordo custom resource
pub fn gordo_custom_resource_api(client: APIClient) -> Api<Gordo> {
    let gordos: Api<Gordo> = Api::customResource(client, "gordos")
        .version("v1")
        .group("equinor.com")
        .within("default");
    gordos
}

// Remove _all_ gordos.
pub async fn remove_gordos(gordos: &Api<Gordo>) {
    for gordo in gordos
        .list(&ListParams::default())
        .await
        .unwrap()
        .items
        .iter()
    {
        gordos
            .delete(&gordo.metadata.name, &DeleteParams::default())
            .await
            .unwrap();
    }
}

// Get the repo's example `Gordo` config file
pub fn gordo_example_config() -> Value {
    let config_str =
        std::fs::read_to_string(format!("{}/example-gordo.yaml", env!("CARGO_MANIFEST_DIR")))
            .expect("Failed to read config file");
    serde_yaml::from_str(&config_str).expect("Unable to parse config file into yaml")
}

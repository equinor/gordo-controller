use failure::_core::time::Duration;
use gordo_controller::crd::gordo::load_gordo_resource;
use gordo_controller::crd::model::{load_model_resource, Model};
use gordo_controller::{load_kube_config, Gordo};
use kube::api::{DeleteParams, PostParams};
use kube::client::APIClient;
use serde_json::Value;

#[tokio::main]
#[test]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Calling /gordos /models /gordos/<name> and /models/<name> will give back nothing before submitting
    let resp: Vec<Gordo> = reqwest::get("http://0.0.0.0:8888/gordos").await?.json().await?;
    assert!(resp.is_empty());

    let resp: Vec<Model> = reqwest::get("http://0.0.0.0:8888/models").await?.json().await?;
    assert!(resp.is_empty());

    let resp: Option<Gordo> = reqwest::get("http://0.0.0.0:8888/gordos/test-project-name")
        .await?
        .json()
        .await?;
    assert!(resp.is_none());

    let resp: Vec<Model> = reqwest::get("http://0.0.0.0:8888/models/test-project-name")
        .await?
        .json()
        .await?;
    assert!(resp.is_empty());

    // Apply a Gordo and Model
    let gordo: Value = read_manifest("example-gordo.yaml");
    let model: Value = read_manifest("example-model.yaml");

    let client = APIClient::new(load_kube_config().await);
    let gordo_api = load_gordo_resource(&client, "default");
    let model_api = load_model_resource(&client, "default");

    // Create the Gordo and Model
    gordo_api
        .create(&PostParams::default(), serde_json::to_vec(&gordo).unwrap())
        .await
        .unwrap();
    model_api
        .create(&PostParams::default(), serde_json::to_vec(&model).unwrap())
        .await
        .unwrap();

    // Wait for controller to pick up changes
    std::thread::sleep(Duration::from_secs(20));

    // Calling /gordos /models /gordos/<name> and /models/<name> will give now give back stuff
    let resp: Vec<Gordo> = reqwest::get("http://0.0.0.0:8888/gordos").await?.json().await?;
    assert_eq!(resp.len(), 1);

    let resp: Vec<Model> = reqwest::get("http://0.0.0.0:8888/models").await?.json().await?;
    assert_eq!(resp.len(), 1);

    let resp: Option<Gordo> = reqwest::get("http://0.0.0.0:8888/gordos/test-project-name")
        .await?
        .json()
        .await?;
    assert!(resp.is_some());

    let resp: Vec<Model> = reqwest::get("http://0.0.0.0:8888/models/test-project-name")
        .await?
        .json()
        .await?;
    assert_eq!(resp.len(), 1);

    // Clean up
    gordo_api
        .delete("test-project-name", &DeleteParams::default())
        .await
        .unwrap();

    Ok(())
}

fn read_manifest(name: &str) -> Value {
    let raw = std::fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), name)).unwrap();
    serde_yaml::from_str(&raw).unwrap()
}

use failure::_core::time::Duration;
use gordo_controller::{
    crd::gordo::{load_gordo_resource, Gordo},
    crd::model::{load_model_resource, Model},
    load_kube_config,
};
use kube::api::{DeleteParams, PostParams};
use kube::client::APIClient;
use serde_json::Value;

#[macro_use]
mod helpers;

#[tokio::main]
#[test]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Calling /gordos /models /gordos/<name> and /models/<name> will give back nothing before submitting
    // Use wait_or_panic because the running controller may still have gordo's in the current state.
    wait_or_panic!({
        let resp: Vec<Gordo> = reqwest::get("http://0.0.0.0:8888/gordos").await?.json().await?;
        if resp.is_empty() {
            break;
        }
    });

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
    let gordo: Gordo = serde_json::from_value(gordo).unwrap();

    let model: Value = read_manifest("example-model.yaml");
    let mut model: Model = serde_json::from_value(model).unwrap();

    let client = APIClient::new(load_kube_config().await);
    let gordo_api = load_gordo_resource(&client, "default");
    let model_api = load_model_resource(&client, "default");

    // Create the Gordo and Model
    gordo_api
        .create(&PostParams::default(), serde_json::to_vec(&gordo).unwrap())
        .await
        .unwrap();
    std::thread::sleep(Duration::from_secs(2));

    // Wait for controller to pick up and assign a status to this gordo, which will have the project revision set
    while let Ok(gordo) = gordo_api.get(&gordo.metadata.name).await {
        match gordo.status {
            Some(status) => {
                // Update this model's project-revision to match the revision number given to the owning Gordo
                model.metadata.labels.insert(
                    "applications.gordo.equinor.com/project-revision".to_string(),
                    status.project_revision,
                );
                break;
            }
            None => std::thread::sleep(Duration::from_secs(2)),
        }
    }

    // Apply the model to the cluster
    model_api
        .create(&PostParams::default(), serde_json::to_vec(&model).unwrap())
        .await
        .unwrap();

    // Calling /gordos /models /gordos/<name> and /models/<name> will now give back stuff
    wait_or_panic!({
        let resp: Vec<Gordo> = reqwest::get("http://0.0.0.0:8888/gordos").await?.json().await?;
        if resp.len() == 1 {
            break;
        }
    });

    wait_or_panic!({
        let resp: Vec<Model> = reqwest::get("http://0.0.0.0:8888/models").await?.json().await?;
        if resp.len() == 1 {
            break;
        }
    });

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
    model_api
        .delete("gordo-model-name", &DeleteParams::default())
        .await
        .unwrap();

    // Wait for the gordo and model to be deleted.
    wait_or_panic!({
        if gordo_api.get("test-project-name").await.is_err() && model_api.get("gordo-model-name").await.is_err() {
            break;
        }
    });

    Ok(())
}

fn read_manifest(name: &str) -> Value {
    let raw = std::fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), name)).unwrap();
    serde_yaml::from_str(&raw).unwrap()
}

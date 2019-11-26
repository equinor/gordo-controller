use kube::config;
use serde_json::Value;
use serde_yaml;

use failure::_core::time::Duration;
use gordo_controller::crd::{gordo::load_gordo_resource, model::load_model_resource};
use kube::api::{DeleteParams, ListParams, PostParams};
use kube::client::APIClient;

#[tokio::main]
#[test]
async fn main() {
    let gordo_raw = std::fs::read_to_string(format!("{}/example-gordo.yaml", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let gordo: Value = serde_yaml::from_str(&gordo_raw).unwrap();

    let client = APIClient::new(config::load_kube_config().await.unwrap());

    let gordo_api = load_gordo_resource(&client, "default");

    // Presently, no gordos, but after submitting one should be available
    assert_eq!(gordo_api.list(&ListParams::default()).await.unwrap().items.len(), 0);
    assert!(gordo_api
        .create(&PostParams::default(), serde_json::to_vec(&gordo).unwrap())
        .await
        .is_ok(),);
    assert_eq!(gordo_api.list(&ListParams::default()).await.unwrap().items.len(), 1);

    // Wait for running controller to update the status
    std::thread::sleep(Duration::from_secs(20));

    // Fetch and check number of models
    let gordo = gordo_api.get("test-project-name").await.unwrap();
    assert_eq!(gordo.spec.config.n_models(), 9);
    assert!(gordo.status.is_some());

    let status = gordo.status.unwrap();
    assert_eq!(status.n_models, 9);
    assert_eq!(status.n_models_built, 0); // no models built yet.

    // simulate a 'finished' model by submitting the example model
    let model_raw = std::fs::read_to_string(format!("{}/example-model.yaml", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let model: Value = serde_yaml::from_str(&model_raw).unwrap();

    let model_api = load_model_resource(&client, "default");
    assert!(model_api
        .create(&PostParams::default(), serde_json::to_vec(&model).unwrap())
        .await
        .is_ok());

    // Wait for controller to pick up the change
    std::thread::sleep(Duration::from_secs(30));

    assert_eq!(model_api.list(&ListParams::default()).await.unwrap().items.len(), 1);

    // Now Gordo's status should report 1 model built
    let gordo = gordo_api.get("test-project-name").await.unwrap();
    assert_eq!(gordo.status.unwrap().n_models_built, 1);

    // Cleanup
    assert!(gordo_api
        .delete("test-project-name", &DeleteParams::default())
        .await
        .is_ok());
    assert!(model_api
        .delete("gordo-model-name", &DeleteParams::default())
        .await
        .is_ok());
}

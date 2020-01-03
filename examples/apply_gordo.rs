use kube::config;
use serde_json::Value;
use serde_yaml;

use gordo_controller::crd::model::Model;
use gordo_controller::crd::{gordo::load_gordo_resource, model::load_model_resource};
use kube::api::{DeleteParams, ListParams, PostParams};
use kube::client::APIClient;
use std::time::{Duration, Instant};

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

    // Fetch and check number of models when controller has updated the status
    let gordo = wait_or_panic!({
        if let Ok(gordo) = gordo_api.get("test-project-name").await {
            if gordo.status.is_some() {
                break gordo;
            }
        }
    });

    assert_eq!(gordo.spec.config.n_models(), 9);

    let status = gordo.status.as_ref().unwrap();
    assert_eq!(status.n_models, 9);
    assert_eq!(status.n_models_built, 0); // no models built yet.

    // simulate a 'finished' model by submitting the example model
    let model_raw = std::fs::read_to_string(format!("{}/example-model.yaml", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let model: Value = serde_yaml::from_str(&model_raw).unwrap();
    let mut model: Model = serde_json::from_value(model).unwrap();

    // Update the model to match the project-revision set by the controller
    model.metadata.labels.insert(
        "applications.gordo.equinor.com/project-revision".to_owned(),
        gordo.status.unwrap().project_revision,
    );

    let model_api = load_model_resource(&client, "default");
    assert!(model_api
        .create(&PostParams::default(), serde_json::to_vec(&model).unwrap())
        .await
        .is_ok());

    // Wait for controller to pick up the change and return models
    let models = wait_or_panic!({
        if let Ok(models) = model_api.list(&ListParams::default()).await {
            break models;
        }
    });
    assert_eq!(models.items.len(), 1);

    // Now Gordo's status should report 1 model built after a while
    wait_or_panic!({
        if let Ok(gordo) = gordo_api.get("test-project-name").await {
            if gordo.status.unwrap().n_models_built == 1 {
                break;
            }
        }
    });

    // Cleanup
    assert!(gordo_api
        .delete("test-project-name", &DeleteParams::default())
        .await
        .is_ok());
    assert!(model_api
        .delete("gordo-model-name", &DeleteParams::default())
        .await
        .is_ok());

    // Wait for both to be deleted.
    wait_or_panic!({
        if gordo_api.get("test-project-name").await.is_err() && model_api.get("gordo-model-name").await.is_err() {
            break;
        }
    });
}

#[macro_export]
macro_rules! wait_or_panic {
    // Execute a block of code in a loop with 1 second waits up to 30 seconds total run time
    // Use: wait_or_panic!({if 5 > 2 { break }})
    ($code:block) => {

        {
            let start = Instant::now();
            loop {

                $code

                if Instant::now() - start > Duration::from_secs(30) {
                    panic!("Timeout waiting for condition");
                } else {
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }

    }
}

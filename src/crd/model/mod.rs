pub mod model;
pub use model::*;

use futures::future::join;
use kube::{api::PatchParams, client::APIClient};
use log::error;
use serde_json::json;

use crate::crd::gordo::{load_gordo_resource, GordoStatus};
use crate::GordoEnvironmentConfig;
use kube::api::Reflector;

pub async fn monitor_models(client: &APIClient, namespace: &str, _env_config: &GordoEnvironmentConfig) -> ! {
    let model_resource = load_model_resource(&client, &namespace);
    let gordo_resource = load_gordo_resource(&client, &namespace);

    let model_reflector = Reflector::new(model_resource.clone()).init().await.unwrap();
    let gordo_reflector = Reflector::new(gordo_resource.clone()).init().await.unwrap();

    loop {
        let models = model_reflector.read().unwrap();
        let gordos = gordo_reflector.read().unwrap();

        // Compare each Gordo's n-models-built against the total models currently found for that Gordo
        for gordo in gordos {
            let n_models_built = models
                .iter()
                .filter(|model| {
                    model
                        .metadata
                        .ownerReferences
                        .iter()
                        .any(|owner_ref| owner_ref.name == gordo.metadata.name)
                })
                .count();

            // If the gordo's current status of built models doesn't match the current models existing
            // we need to patch its status to reflect the actual models built for it.
            if gordo.status.clone().unwrap_or_default().n_models_built != n_models_built {
                let mut status = GordoStatus::from(&gordo);
                status.n_models_built = n_models_built;

                let patch = serde_json::to_vec(&json!({ "status": status })).unwrap();
                let pp = PatchParams::default();

                if let Err(err) = gordo_resource.patch_status(&gordo.metadata.name, &pp, patch).await {
                    error!(
                        "Failed to patch status of Gordo '{}' - error: {:?}",
                        &gordo.metadata.name, err
                    );
                }
            }
        }

        // Poll reflectors simultaneously
        let (result1, result2) = join(model_reflector.poll(), gordo_reflector.poll()).await;
        if let Err(err) = result1 {
            error!("Failed polling model reflector with error: {:?}", err);
        }
        if let Err(err) = result2 {
            error!("Failed polling Gordo reflector with error: {:?}", err);
        }
    }
}

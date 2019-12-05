pub mod model;
pub use model::*;

use kube::api::PatchParams;
use log::error;
use serde_json::json;

use crate::crd::gordo::GordoStatus;
use crate::Controller;

pub async fn monitor_models(controller: &Controller) -> () {
    let models = controller.model_state().await;
    let gordos = controller.gordo_state().await;

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

            if let Err(err) = controller
                .gordo_resource
                .patch_status(&gordo.metadata.name, &pp, patch)
                .await
            {
                error!(
                    "Failed to patch status of Gordo '{}' - error: {:?}",
                    &gordo.metadata.name, err
                );
            }
        }
    }
}

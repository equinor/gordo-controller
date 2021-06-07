pub mod model;
pub use model::*;

use kube::api::{Api, PatchParams};
use log::{error, info};
use serde_json::json;

use crate::crd::gordo::GordoStatus;
use crate::Controller;
use crate::crd::metrics::{kube_error_happened};

pub async fn patch_model_with_default_status<'a>(model_resource: &'a Api<Model>, model: &'a Model) -> Result<Model, kube::Error>{
    let mut status = ModelStatus::default();
    status.revision = match model.metadata.labels.get("applications.gordo.equinor.com/project-revision") {
        Some(revision) => Some(revision.to_string()),
        None => None,
    };
    patch_model_status(model_resource, &model.metadata.name, status.clone()).await
}

pub async fn monitor_models(controller: &Controller) -> () {
    let models = controller.model_state().await;
    let gordos = controller.gordo_state().await;

    for model in &models {
        if let None = model.status {
            //TODO Update state here
            //let name = model.spec.config["name"].as_str().unwrap_or("unknown");
            info!("Unknown status for model {}", model.metadata.name);
            match patch_model_with_default_status(&controller.model_resource, &model).await {
                Ok(new_model) => info!("Patching Model '{}' from status {:?} to {:?}", model.metadata.name, model.status, new_model.status),
                Err(err) => {
                  error!( "Failed to patch status of Model '{}' - error: {:?}", model.metadata.name, err);
                  kube_error_happened("patch_gordo", err);
                }
            }
        }
    }

    // Compare each Gordo's n-models-built against the total models currently found for that Gordo
    for gordo in gordos {
        let n_models_built = filter_models_on_gordo(&gordo, &models)
            .filter(|model| match model.status.as_ref() {
                Some(status) => status.phase == ModelPhase::Succeeded,
                None => false,
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
                kube_error_happened("patch_gordo", err);
            }
        }
    }
}

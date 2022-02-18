pub mod model;
pub use model::*;

use kube::api::{Api, PatchParams, Patch};
use log::{error, info};
use serde_json::json;

use crate::crd::gordo::gordo::{Gordo, GordoStatus};
use crate::crd::metrics::{kube_error_happened, MODEL_PULLING};
use crate::errors::Error;

pub async fn patch_model_with_default_status<'a>(model_resource: &'a Api<Model>, model: &'a Model) -> Result<Model, Error>{
    let mut status = ModelStatus::default();
    status.revision = match model.metadata.labels.get("applications.gordo.equinor.com/project-revision") {
        Some(revision) => Some(revision.to_string()),
        None => None,
    };
    match model.metadata.name {
        Some(name) => patch_model_status(
            model_resource,
            &name,
            &status
        ).await.map_err(Error::KubeError),
        None => Err(Error::MissingKey(".metadata.name")),
    }
}

pub async fn monitor_models(model_api: &Api<Model>, gordo_api: &Api<Gordo>, models: &Vec<Model>, gordos: &Vec<Gordo>) -> () {
    for model in &models.iter() {
        if let None = model.status {
            //TODO Update state here
            //let name = model.spec.config["name"].as_str().unwrap_or("unknown");
            info!("Unknown status for model {}", model.metadata.name);
            match patch_model_with_default_status(model_api, &model).await {
                Ok(new_model) => info!("Patching Model '{}' from status {:?} to {:?}", model.metadata.name, model.status, new_model.status),
                Err(err) => {
                    error!( "Failed to patch status of Model '{}' - error: {:?}", model.metadata.name, err);
                    match err {
                        Error::KubeError(kube_error) =>
                            kube_error_happened("patch_gordo", kube_error),
                        _ => { },
                    }
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

            if let Err(err) = gordo_api
                .patch_status(&gordo.metadata.name, &pp, &Patch::Merge(patch))
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
    MODEL_PULLING.with_label_values(&[]).inc();
}

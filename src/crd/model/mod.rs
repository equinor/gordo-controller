pub mod model;
pub use model::*;

use kube::{
    api::{Api, Informer, PatchParams, WatchEvent},
    client::APIClient,
};
use log::{error, info, warn};
use serde_json::json;

use crate::crd::gordo::{load_gordo_resource, GordoStatus};
use crate::GordoEnvironmentConfig;
use kube::api::ListParams;

pub async fn monitor_models(client: &APIClient, namespace: &str, _env_config: &GordoEnvironmentConfig) -> ! {
    let model_resource: Api<Model> = Api::customResource(client.clone(), "models")
        .version("v1")
        .group("equinor.com")
        .within(&namespace);
    let model_informer: Informer<Model> = Informer::new(model_resource.clone()).init().await.unwrap();

    let mut outdated_version = false;
    loop {
        // updates to models
        model_informer
            .poll()
            .await
            .unwrap_or_else(|e| panic!("Failed to poll model informer: {:?}", e));

        while let Some(event) = model_informer.pop() {
            if let Err(err) = handle_model_event(&client, event, &model_resource, &namespace).await {
                error!("Watch event error for model informer: {:?}", err);
                outdated_version = true;
            };
        }

        // Reset the informer if an error was encountred.
        if outdated_version {
            model_informer.reset().await.unwrap();
            outdated_version = false;
        }
    }
}

/// Do what needs to be done with a model event
async fn handle_model_event(
    client: &APIClient,
    event: WatchEvent<Model>,
    resource: &Api<Model>,
    namespace: &str,
) -> Result<(), kube::ApiError> {
    match event {
        WatchEvent::Added(model) => {
            info!("New gordo model: {:?} - {:?}", model.metadata.name, model.status);
            if let Err(err) = update_gordo_models_build_count(&model, &client, &resource, &namespace).await {
                error!(
                    "Failed to update Gordo.n-models-built count for Model '{}' with error: {:?}",
                    &model.metadata.name, err
                );
            };
        }
        WatchEvent::Modified(model) => {
            info!("Modified gordo model: {:?} - {:?}", model.metadata.name, model.status);
        }
        WatchEvent::Deleted(model) => info!("Deleted gordo model: {:?} - {:?}", model.metadata.name, model.status),
        WatchEvent::Error(err) => return Err(err),
    }
    Ok(())
}

/// Given a `Model`, update the associated `Gordo` with the current number of models
/// which are built for it.
async fn update_gordo_models_build_count(
    model: &Model,
    client: &APIClient,
    resource: &Api<Model>,
    namespace: &str,
) -> Result<(), kube::Error> {
    match model.metadata.labels.get("applications.gordo.equinor.com/project-name") {
        Some(project_name) => {
            // Find the owning Gordo, which is the project name
            let gordo_api = load_gordo_resource(&client, &namespace);
            let gordo = gordo_api.get(project_name).await?;

            // Get all models for this Gordo
            let mut lp = ListParams::default();
            lp.label_selector = Some(format!("applications.gordo.equinor.com/project-name={}", &project_name));
            let models = resource.list(&lp).await?;

            // Create a status from the current status, and then update the number of built models
            let mut status = GordoStatus::from(&gordo);
            status.n_models_built = models.items.len();
            let patch = serde_json::to_vec(&json!({ "status": status })).unwrap();

            // Patch the status with the current models built
            gordo_api
                .patch_status(&gordo.metadata.name, &PatchParams::default(), patch)
                .await?;
        }
        None => warn!(
            "Model {} did not have 'applications.gordo.equinor.com/project-name' defined, cannot update owning Gordo.",
            &model.metadata.name
        ),
    }
    Ok(())
}

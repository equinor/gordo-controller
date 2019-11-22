pub mod model;
pub use model::*;

use kube::{
    api::{Api, Informer, PatchParams, WatchEvent},
    client::APIClient,
};
use log::{error, info};
use serde_json::json;

use crate::GordoEnvironmentConfig;

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
            if let Err(err) = handle_model_event(event, &model_resource).await {
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
async fn handle_model_event(event: WatchEvent<Model>, resource: &Api<Model>) -> Result<(), kube::ApiError> {
    match event {
        WatchEvent::Added(model) => {
            info!("New gordo model: {:?} - {:?}", model.metadata.name, model.status);
            let status = json!({ "status": ModelStatus::default() });
            resource
                .patch_status(
                    &model.metadata.name,
                    &PatchParams::default(),
                    serde_json::to_vec(&status).unwrap(),
                )
                .await
                .expect("Failed to patch model status!");
        }
        WatchEvent::Modified(model) => {
            info!("Modified gordo model: {:?} - {:?}", model.metadata.name, model.status);
        }
        WatchEvent::Deleted(model) => info!("Deleted gordo model: {:?} - {:?}", model.metadata.name, model.status),
        WatchEvent::Error(err) => return Err(err),
    }
    Ok(())
}

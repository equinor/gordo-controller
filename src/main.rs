use crate::deploy_job::DeployJob;

use kube::{
    api::{Api, WatchEvent, Informer, PatchParams},
    client::APIClient,
    config,
};
use log::{error, info};
use serde::Deserialize;
use serde_json::json;

mod crd;
mod deploy_job;
#[cfg(test)]
mod tests;

use crate::crd::gordo::{Gordo, GordoStatus};
use crate::crd::model::{Model, ModelStatus};

#[derive(Deserialize, Debug, Clone)]
pub struct GordoEnvironmentConfig {
    deploy_image: String,
}
impl Default for GordoEnvironmentConfig {
    fn default() -> Self {
        GordoEnvironmentConfig {
            deploy_image: "auroradevacr.azurecr.io/gordo-infrastructure/gordo-deploy".to_owned(),
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

async fn handle_gordo_event(
    event: WatchEvent<Gordo>,
    client: &APIClient,
    resource: &Api<Gordo>,
    namespace: &str,
    env_config: &GordoEnvironmentConfig,
) -> Result<(), kube::ApiError> {
    match event {
        WatchEvent::Added(gordo) => {
            crate::crd::gordo::start_gordo_deploy_job(&gordo, &client, &resource, &namespace, &env_config).await;
        }
        WatchEvent::Modified(gordo) => {
            info!(
                "Gordo resource modified: {:?}, status is: {:?}",
                &gordo.metadata.name, &gordo.status
            );
            match gordo.status {
                Some(ref status) => {
                    match status {
                        GordoStatus::Submitted(ref generation) => {
                            // If it's submitted, we only want to launch the job if the GenerationNumber has changed.
                            if generation != &gordo.metadata.generation.map(|v| v as u32) {
                                crate::crd::gordo::start_gordo_deploy_job(&gordo, &client, &resource, &namespace, &env_config).await;
                            }
                        }
                    }
                }

                // No Gordo status
                None => {
                    crate::crd::gordo::start_gordo_deploy_job(&gordo, &client, &resource, &namespace, &env_config).await;
                }
            }
        }
        WatchEvent::Deleted(gordo) => {
            info!("Gordo resource deleted: {:?}", gordo.metadata.name);

            // Remove any old jobs associated with this Gordo which has been deleted.
            crate::crd::gordo::remove_gordo_deploy_jobs(&gordo, &client, &namespace).await;
        }
        WatchEvent::Error(err) => return Err(err),
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ! {
    std::env::set_var("RUST_LOG", "info,kube=info");
    env_logger::init();

    // Load environment variables
    let env_config = envy::from_env::<GordoEnvironmentConfig>().unwrap_or_else(|e| {
        error!("Failed to load environment config, using defaults: {:?}", e);
        GordoEnvironmentConfig::default()
    });

    let kube_config = config::load_kube_config()
        .await
        .unwrap_or_else(|_| config::incluster_config().expect("Failed to get local kube config and incluster config"));

    let namespace = kube_config.default_ns.to_owned();
    info!("Got default namespace of: {}", &namespace);

    let client = APIClient::new(kube_config);

    let gordo_resource: Api<Gordo> = Api::customResource(client.clone(), "gordos")
        .version("v1")
        .group("equinor.com")
        .within(&namespace);
    let gordo_informer: Informer<Gordo> = Informer::new(gordo_resource.clone()).init().await.unwrap();

    let model_resource: Api<Model> = Api::customResource(client.clone(), "models")
        .version("v1")
        .group("equinor.com")
        .within(&namespace);
    let model_informer: Informer<Model> = Informer::new(model_resource.clone()).init().await.unwrap();

    // On start up, get a list of all gordos, and start gordo-deploy jobs for each
    // which doesn't have a Submitted(revision) which doesn't match its current revision
    // or otherwise hasn't been submitted at all.
    crate::crd::gordo::launch_waiting_gordo_workflows(&gordo_resource, &client, &namespace, &env_config).await;

    loop {
        // updates to models
        model_informer
            .poll()
            .await
            .unwrap_or_else(|e| panic!("Failed to poll model informer: {:?}", e));

        while let Some(event) = model_informer.pop() {
            if let Err(err) = handle_model_event(event, &model_resource).await {
                error!("Watch event error for model informer: {:?}", err);
                model_informer.reset().await.unwrap();
            };
        }

        // Update state changes
        gordo_informer
            .poll()
            .await
            .unwrap_or_else(|e| panic!("Failed to poll: {:?}", e));

        while let Some(event) = gordo_informer.pop() {
            if let Err(err) = handle_gordo_event(event, &client, &gordo_resource, &namespace, &env_config).await {
                error!("Watch event error for gordo informer: {:?}", err);
                gordo_informer.reset().await.unwrap();
            }
        }
    }
}

// Get a minor version from standard SemVer string
pub fn minor_version(deploy_version: &str) -> Option<u32> {
    deploy_version
        .split('.')
        .nth(1)
        .map(|v| v.parse::<u32>().ok())
        .unwrap_or(None)
}

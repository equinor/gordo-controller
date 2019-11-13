use crate::deploy_job::DeployJob;
use futures::future::join_all;
use kube::api::{DeleteParams, Informer, ListParams, PatchParams};
use kube::{
    api::{Api, Object, PostParams, WatchEvent},
    client::APIClient,
    config,
};
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

mod deploy_job;
#[cfg(test)]
mod tests;

type GenerationNumber = Option<u32>;
type Gordo = Object<GordoSpec, GordoStatus>;

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

/// Represents the 'spec' field of a Gordo resource
#[derive(Serialize, Deserialize, Clone)]
pub struct GordoSpec {
    #[serde(rename = "deploy-version")]
    deploy_version: String,
    #[serde(rename = "deploy-environment")]
    deploy_environment: Option<HashMap<String, String>>,
    config: Value,
}

/// Represents the possible 'status' of a Gordo resource
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum GordoStatus {
    Submitted(GenerationNumber),
}

impl Default for GordoStatus {
    fn default() -> GordoStatus {
        GordoStatus::Submitted(None)
    }
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

    let kube_config = config::load_kube_config().await.unwrap_or_else(|_| {
        config::incluster_config().expect("Failed to get local kube config and incluster config")
    });

    let namespace = kube_config.default_ns.to_owned();
    info!("Got default namespace of: {}", &namespace);

    let client = APIClient::new(kube_config);

    let resource: Api<Gordo> = Api::customResource(client.clone(), "gordos")
        .version("v1")
        .group("equinor.com")
        .within(&namespace);

    let informer: Informer<Gordo> = Informer::new(resource.clone()).init().await.unwrap();

    // On start up, get a list of all gordos, and start gordo-deploy jobs for each
    // which doesn't have a Submitted(revision) which doesn't match its current revision
    // or otherwise hasn't been submitted at all.
    launch_waiting_gordo_workflows(&resource, &client, &namespace, &env_config).await;

    loop {

        // Update state changes
        informer
            .poll()
            .await
            .unwrap_or_else(|e| panic!("Failed to poll: {:?}", e));

        while let Some(event) = informer.pop() {
            match event {
                WatchEvent::Added(gordo) => {
                    start_gordo_deploy_job(&gordo, &client, &resource, &namespace, &env_config)
                        .await;
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
                                        start_gordo_deploy_job(
                                            &gordo,
                                            &client,
                                            &resource,
                                            &namespace,
                                            &env_config,
                                        )
                                        .await;
                                    }
                                }
                            }
                        }

                        // No Gordo status
                        None => {
                            start_gordo_deploy_job(
                                &gordo,
                                &client,
                                &resource,
                                &namespace,
                                &env_config,
                            )
                            .await;
                        }
                    }
                }
                WatchEvent::Deleted(gordo) => {
                    info!("Gordo resource deleted: {:?}", gordo.metadata.name);

                    // Remove any old jobs associated with this Gordo which has been deleted.
                    remove_gordo_deploy_jobs(&gordo, &client, &namespace).await;
                }
                WatchEvent::Error(e) => info!("Gordo resource error from k8s: {:?}", e),
            }
        }
    }
}

/// Look for and submit `Gordo`s which have a `GenerationNumber` different than what Kubernetes
/// has set in its `metadata.generation`; meaning changes have been submitted to the resource but
/// the `GordoStatus` has not been updated to reflect this and therefore needs to be submitted to
/// the workflow generator job.
pub(crate) async fn launch_waiting_gordo_workflows(
    resource: &Api<Gordo>,
    client: &APIClient,
    namespace: &str,
    env_config: &GordoEnvironmentConfig,
) -> () {
    match resource.list(&ListParams::default()).await {
        Ok(gordos) => {
            let n_gordos = gordos.items.len();
            if n_gordos == 0 {
                info!("No waiting gordos need submitting.");
            } else {
                info!(
                    "Found {} gordos, checking if any need submitting to gordo-deploy",
                    n_gordos
                );

                join_all(
                    gordos
                        .items
                        .iter()
                        .filter(|gordo| {
                            // Determine if this gordo should be submitted to gordo-deploy
                            match &gordo.status {
                                Some(status) => {
                                    match status {
                                        // Already submitted; only re-submit if the revision has changed from the one submitted.
                                        GordoStatus::Submitted(revision) => {
                                            revision != &gordo.metadata.generation.map(|v| v as u32)
                                        }
                                    }
                                }
                                None => true, // No status, should submit
                            }
                        })
                        .map(|gordo| {
                            // Submit this gordo resource.
                            info!("Submitting waiting Gordo: {}", &gordo.metadata.name);
                            start_gordo_deploy_job(
                                gordo,
                                &client,
                                &resource,
                                &namespace,
                                &env_config,
                            )
                        }),
                )
                .await;
            }
        }
        Err(e) => error!("Unable to list previous gordos: {:?}", e),
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

/// Start a gordo-deploy job using this `Gordo`.
/// Will patch the status of the `Gordo` to reflect the current revision number
async fn start_gordo_deploy_job(
    gordo: &Gordo,
    client: &APIClient,
    resource: &Api<Gordo>,
    namespace: &str,
    env_config: &GordoEnvironmentConfig,
) -> () {
    // Job manifest for launching this gordo config into a workflow
    let job = DeployJob::new(&gordo, &env_config);

    // Before launching this job, remove previous jobs for this project
    remove_gordo_deploy_jobs(&gordo, &client, &namespace).await;

    // Send off job, later we can add support to watching the job if needed via `jobs.watch(..)`
    info!("Launching job - {}!", &job.name);
    let postparams = PostParams::default();
    let jobs = Api::v1Job(client.clone()).within(&namespace);

    let serialized_job_manifest = job.as_vec();
    match jobs.create(&postparams, serialized_job_manifest).await {
        Ok(job) => info!("Submitted job: {:?}", job.metadata.name),
        Err(e) => error!("Failed to submit job with error: {:?}", e),
    }

    // Update the status of this job
    info!(
        "Setting status of this gordo '{:?}' to 'Submitted'",
        &gordo.metadata.name
    );
    let status = json!({
        "status": GordoStatus::Submitted(gordo.metadata.generation.map(|v| v as u32))
    });
    match resource
        .patch_status(
            &gordo.metadata.name,
            &PatchParams::default(),
            serde_json::to_vec(&status).expect("Status was not serializable, should never happen."),
        )
        .await
    {
        Ok(o) => info!("Patched status: {:?}", o.status),
        Err(e) => error!("Failed to patch status: {:?}", e),
    };
}

/// Remove any gordo deploy jobs associated with this `Gordo`
pub(crate) async fn remove_gordo_deploy_jobs(
    gordo: &Gordo,
    client: &APIClient,
    namespace: &str,
) -> () {
    info!(
        "Removing any gordo-deploy jobs for Gordo: '{}'",
        &gordo.metadata.name
    );

    let jobs = Api::v1Job(client.clone()).within(&namespace);
    match jobs.list(&ListParams::default()).await {
        Ok(job_list) => {
            join_all(job_list
                .items
                .into_iter()
                .filter(|job| {
                    job.metadata.labels.get("gordoProjectName") == Some(&gordo.metadata.name)
                })
                .map(|job| async move {
                    let jobs_api = Api::v1Job(client.clone()).within(&namespace);
                    match jobs_api
                        .delete(&job.metadata.name, &DeleteParams::default())
                        .await
                    {
                        Ok(_) => {
                            info!(
                                "Successfully requested to delete job: {}, waiting for it to die.",
                                &job.metadata.name
                            );

                            // Keep trying to get the job, it will fail when it no longer exists.
                            while let Ok(job) = jobs_api.get(&job.metadata.name).await {
                                info!(
                                    "Got job resourceVersion: {:#?}, generation: {:#?} waiting for it to be deleted.",
                                    job.metadata.resourceVersion, job.metadata.generation
                                );
                                std::thread::sleep(std::time::Duration::from_secs(1));
                            }
                        }
                        Err(err) => error!(
                            "Failed to delete old gordo job: '{}' with error: {:?}",
                            &job.metadata.name, err
                        ),
                    }
            })).await;
        }
        Err(e) => error!("Failed to list jobs: {:?}", e),
    }
}

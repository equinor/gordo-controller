use futures::future::join_all;
use kube::{
    api::{Api, DeleteParams, ListParams, Object, PatchParams, PostParams},
    client::APIClient,
};
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::{DeployJob, GordoEnvironmentConfig};

pub type GenerationNumber = Option<u32>;
pub type Gordo = Object<GordoSpec, GordoStatus>;

/// Represents the 'spec' field of a Gordo resource
#[derive(Serialize, Deserialize, Clone)]
pub struct GordoSpec {
    #[serde(rename = "deploy-version")]
    pub deploy_version: String,
    #[serde(rename = "deploy-environment")]
    pub deploy_environment: Option<HashMap<String, String>>,
    pub config: GordoConfig,
}

/// The actual structure, so much as we need to parse, of a gordo config.
#[derive(Serialize, Deserialize, Clone)]
pub struct GordoConfig {
    #[serde(alias = "machines")]
    models: Vec<Value>,
    #[serde(default)]
    globals: Option<Value>,
}

impl GordoConfig {
    /// Count of models defined in this config
    pub fn n_models(&self) -> usize {
        self.models.len()
    }
}

/// Load the `Gordo` custom resource API interface
pub fn load_gordo_resource(client: &APIClient, namespace: &str) -> Api<Gordo> {
    Api::customResource(client.clone(), "gordos")
        .version("v1")
        .group("equinor.com")
        .within(&namespace)
}

/// Represents the possible 'status' of a Gordo resource
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GordoStatus {
    #[serde(rename = "n-models", default)]
    pub n_models: usize,
    #[serde(rename = "submission-status", default)]
    pub submission_status: GordoSubmissionStatus,
    #[serde(rename = "n-models-built", default)]
    pub n_models_built: usize,
}

impl From<&Gordo> for GordoStatus {
    fn from(gordo: &Gordo) -> Self {
        let submission_status = GordoSubmissionStatus::Submitted(gordo.metadata.generation.map(|v| v as u32));
        let gordo_status = gordo.status.clone().unwrap_or_default();
        Self {
            submission_status,
            n_models: gordo.spec.config.n_models(),
            n_models_built: gordo_status.n_models_built,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum GordoSubmissionStatus {
    Submitted(GenerationNumber),
}
impl Default for GordoSubmissionStatus {
    fn default() -> GordoSubmissionStatus {
        GordoSubmissionStatus::Submitted(None)
    }
}

/// Look for and submit `Gordo`s which have a `GenerationNumber` different than what Kubernetes
/// has set in its `metadata.generation`; meaning changes have been submitted to the resource but
/// the `GordoStatus` has not been updated to reflect this and therefore needs to be submitted to
/// the workflow generator job.
pub async fn launch_waiting_gordo_workflows(
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
                                    match status.submission_status {
                                        // Already submitted; only re-submit if the revision has changed from the one submitted.
                                        GordoSubmissionStatus::Submitted(revision) => {
                                            revision != gordo.metadata.generation.map(|v| v as u32)
                                        }
                                    }
                                }
                                None => true, // No status, should submit
                            }
                        })
                        .map(|gordo| {
                            // Submit this gordo resource.
                            info!("Submitting waiting Gordo: {}", &gordo.metadata.name);
                            start_gordo_deploy_job(gordo, &client, &resource, &namespace, &env_config)
                        }),
                )
                .await;
            }
        }
        Err(e) => error!("Unable to list previous gordos: {:?}", e),
    }
}

/// Start a gordo-deploy job using this `Gordo`.
/// Will patch the status of the `Gordo` to reflect the current revision number
pub async fn start_gordo_deploy_job(
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

    let status = json!({ "status": GordoStatus::from(gordo) });

    // Update the status of this job
    info!(
        "Setting status of this gordo '{}' to '{:?}'",
        &gordo.metadata.name, &status
    );
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
pub async fn remove_gordo_deploy_jobs(gordo: &Gordo, client: &APIClient, namespace: &str) -> () {
    info!("Removing any gordo-deploy jobs for Gordo: '{}'", &gordo.metadata.name);

    let jobs = Api::v1Job(client.clone()).within(&namespace);
    match jobs.list(&ListParams::default()).await {
        Ok(job_list) => {
            join_all(
                job_list
                    .items
                    .into_iter()
                    .filter(|job| job.metadata.labels.get("gordoProjectName") == Some(&gordo.metadata.name))
                    .map(|job| {
                        async move {
                            let jobs_api = Api::v1Job(client.clone()).within(&namespace);
                            match jobs_api.delete(&job.metadata.name, &DeleteParams::default()).await {
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
                                        tokio::timer::delay_for(std::time::Duration::from_secs(1)).await;
                                    }
                                }
                                Err(err) => error!(
                                    "Failed to delete old gordo job: '{}' with error: {:?}",
                                    &job.metadata.name, err
                                ),
                            }
                        }
                    }),
            )
                .await;
        }
        Err(e) => error!("Failed to list jobs: {:?}", e),
    }
}

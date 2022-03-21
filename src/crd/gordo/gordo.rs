use futures::future::join_all;
use kube::{
    api::{Api, DeleteParams, ListParams, PatchParams, PostParams, Patch},
    client::Client,
    CustomResource,
};
use k8s_openapi::{
    api::batch::v1::Job,
};
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use schemars::JsonSchema;

use crate::{create_deploy_job, Config};
use crate::crd::metrics::KUBE_ERRORS;
use crate::utils::get_revision;

pub type GenerationNumber = Option<u32>;

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct GordoConfig {
    #[serde(alias = "machines", default)]
    models: Vec<Value>,
    #[serde(default)]
    globals: Option<Value>,
}

#[derive(CustomResource, Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[kube(group = "equinor.com", version = "v1", kind = "Gordo", status="GordoStatus", namespaced)]
#[kube(shortname = "gd")]
pub struct ConfigMapGeneratorSpec {
    #[serde(rename = "deploy-version")]
    pub deploy_version: String,
    #[serde(rename = "deploy-environment")]
    pub deploy_environment: Option<HashMap<String, String>>,
    #[serde(rename = "deploy-repository")]
    pub deploy_repository: Option<String>,
    #[serde(rename = "docker-registry")]
    pub docker_registry: Option<String>,
    #[serde(rename = "debug-show-workflow")]
    pub debug_show_workflow: Option<bool>,
    pub config: GordoConfig,
}

impl GordoConfig {
    /// Count of models defined in this config
    pub fn n_models(&self) -> usize {
        self.models.len()
    }
}

/// Represents the possible 'status' of a Gordo resource
#[derive(Serialize, Deserialize, Clone, Debug, Default, JsonSchema)]
pub struct GordoStatus {
    #[serde(rename = "n-models", default)]
    pub n_models: usize,
    #[serde(rename = "submission-status", default)]
    pub submission_status: GordoSubmissionStatus,
    #[serde(rename = "n-models-built", default)]
    pub n_models_built: usize,
    #[serde(rename = "project-revision", default)]
    pub project_revision: String,
}

impl From<&Gordo> for GordoStatus {
    fn from(gordo: &Gordo) -> Self {
        let submission_status = GordoSubmissionStatus::Submitted(gordo.metadata.generation.map(|v| v as u32));
        let gordo_status = gordo.status.clone().unwrap_or_default();
        Self {
            submission_status,
            n_models: gordo.spec.config.n_models(),
            n_models_built: gordo_status.n_models_built,
            project_revision: gordo_status.project_revision,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
pub enum GordoSubmissionStatus {
    Submitted(GenerationNumber),
}
impl Default for GordoSubmissionStatus {
    fn default() -> GordoSubmissionStatus {
        GordoSubmissionStatus::Submitted(None)
    }
}

/// Start a gordo-deploy job using this `Gordo`.
/// Will patch the status of the `Gordo` to reflect the current revision number
pub async fn start_gordo_deploy_job(
    gordo: &Gordo,
    client: &Client,
    resource: &Api<Gordo>,
    namespace: &str,
    config: &Config,
) -> () {
    // Job manifest for launching this gordo config into a workflow
    let revision = get_revision();
    let gordo_name = gordo.metadata.name.to_owned().unwrap().to_owned();
    let created_job = create_deploy_job(&gordo, &config);
    let job = match created_job {
        Some(job) => job,
        None => {
            error!("Job is empty");
            return
        }
    };

    // Before launching this job, remove previous jobs for this project
    remove_gordo_deploy_jobs(&gordo, &client, &namespace).await;

    let job_name = job.metadata.name.to_owned().unwrap();
    // Send off job, later we can add support to watching the job if needed via `jobs.watch(..)`
    info!("Launching job - {}!", job_name);
    let postparams = PostParams::default();
    let jobs: Api<Job> = Api::namespaced(client.clone(), &namespace);

    match jobs.create(&postparams, &job).await {
        Ok(job) => info!("Submitted job: {:?}", job.metadata.name),
        Err(e) => {
          error!("Failed to submit job with error: {:?}", e);
        }
    }

    let mut status = GordoStatus::from(gordo);
    status.project_revision = revision;

    // Update the status of this job
    info!(
        "Setting status of this gordo '{}' to '{:?}'",
        &gordo_name, &status
    );
    let patch = json!({ "status": status });
    match resource
        .patch_status(&job_name, &PatchParams::default(), &Patch::Merge(patch))
        .await
    {
        Ok(o) => info!("Patched status: {:?}", o.status),
        Err(e) => {
          error!("Failed to patch status: {:?}", e);
        }
    };
}

/// Remove any gordo deploy jobs associated with this `Gordo`
pub async fn remove_gordo_deploy_jobs(gordo: &Gordo, client: &Client, namespace: &str) -> () {
    let gordo_name = gordo.metadata.name.to_owned().unwrap();
    info!("Removing any gordo-deploy jobs for Gordo: '{}'", &gordo_name);

    let jobs: Api<Job> = Api::namespaced(client.clone(), &namespace);
    match jobs.list(&ListParams::default()).await {
        Ok(job_list) => {
            join_all(
                job_list
                    .items
                    .into_iter()
                    .filter(move |job| match &job.metadata.labels {
                        Some(labels) => match labels.get("gordoProjectName") {
                            Some(project_name) => project_name == &gordo_name,
                            None => false,
                        },
                        None => false,
                    })
                    .map(|job| {
                        async move {
                            let jobs_api: Api<Job> = Api::namespaced(client.clone(), &namespace);
                            if let Some(name) = job.metadata.name {
                                match jobs_api.delete(&name, &DeleteParams::default()).await {
                                    Ok(_) => {
                                        info!(
                                            "Successfully requested to delete job: {}, waiting for it to die.",
                                            name
                                        );

                                        // Keep trying to get the job, it will fail when it no longer exists.
                                        while let Ok(job) = jobs_api.get(&name).await {
                                            info!(
                                                "Got job resourceVersion: {:#?}, generation: {:#?} waiting for it to be deleted.",
                                                job.metadata.resource_version, job.metadata.generation
                                            );
                                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                        }
                                    }
                                    Err(err) => {
                                        error!(
                                            "Failed to delete old gordo job: '{}' with error: {:?}",
                                            name, err
                                        );
                                    }
                                }
                            } else {
                                error!("Job does not have .metadata.name");
                                KUBE_ERRORS.with_label_values(&["delete_gordo", "empty_name"]).inc_by(1);
                            }
                        }
                    }),
            )
                .await;
        }
        Err(e) => {
          error!("Failed to list jobs: {:?}", e);
        }
    }
}

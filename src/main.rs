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

fn main() -> ! {
    std::env::set_var("RUST_LOG", "info,kube=info");
    env_logger::init();

    // Load environment variables
    let env_config = envy::from_env::<GordoEnvironmentConfig>().unwrap_or_else(|e| {
        error!("Failed to load environment config, using defaults: {:?}", e);
        GordoEnvironmentConfig::default()
    });

    let kube_config = config::load_kube_config().unwrap_or_else(|_| {
        config::incluster_config().expect("Failed to get local kube config and incluster config")
    });
    let client = APIClient::new(kube_config);

    let namespace =
        std::fs::read_to_string("/var/run/secrets/kubernetes.io/serviceaccount/namespace")
            .expect("Failed to get namespace from system.");

    let resource: Api<Gordo> = Api::customResource(client.clone(), "gordos")
        .version("v1")
        .group("equinor.com")
        .within(&namespace);

    let informer: Informer<Gordo> = Informer::new(resource.clone());

    // On start up, get a list of all gordos, and start gordo-deploy jobs for each
    // which doesn't have a Submitted(revision) which doesn't match its current revision
    // or otherwise hasn't been submitted at all.
    launch_waiting_gordo_workflows(&resource, &client, &namespace, &env_config);

    loop {
        // Update state changes
        informer
            .poll()
            .unwrap_or_else(|e| panic!("Failed to poll: {:?}", e));

        while let Some(event) = informer.pop() {
            match event {
                WatchEvent::Added(gordo) => {
                    start_gordo_deploy_job(&gordo, &client, &resource, &namespace, &env_config)
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
                                        );
                                    }
                                }
                            }
                        }

                        // No Gordo status
                        None => start_gordo_deploy_job(
                            &gordo,
                            &client,
                            &resource,
                            &namespace,
                            &env_config,
                        ),
                    }
                }
                WatchEvent::Deleted(gordo) => {
                    info!("Gordo resource deleted: {:?}", gordo.metadata.name);

                    // Remove any old jobs associated with this Gordo which has been deleted.
                    remove_gordo_deploy_jobs(&gordo, &client, &namespace);
                }
                WatchEvent::Error(e) => info!("Gordo resource error: {:?}", e),
            }
        }
    }
}

/// Look for and submit `Gordo`s which have a `GenerationNumber` different than what Kubernetes
/// has set in its `metadata.generation`; meaning changes have been submitted to the resource but
/// the `GordoStatus` has not been updated to reflect this and therefore needs to be submitted to
/// the workflow generator job.
pub(crate) fn launch_waiting_gordo_workflows(
    resource: &Api<Gordo>,
    client: &APIClient,
    namespace: &str,
    env_config: &GordoEnvironmentConfig,
) -> () {
    match resource.list(&ListParams::default()) {
        Ok(gordos) => {
            let n_gordos = gordos.items.len();
            if n_gordos == 0 {
                info!("No waiting gordos need submitting.");
            } else {
                info!("Found {} gordos, submitting them to gordo-deploy", n_gordos);
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
                    .for_each(|gordo| {
                        // Submit this gordo resource.
                        start_gordo_deploy_job(gordo, &client, &resource, &namespace, &env_config)
                    })
            }
        }
        Err(e) => error!("Unable to list previous gordos: {:?}", e),
    }
}

// Get a minor version from standard SemVer string
pub fn minor_version(deploy_version: &str) -> Option<u32> {
    deploy_version
        .split('.')
        .skip(1)
        .take(1)
        .map(|v| v.parse::<u32>().ok())
        .last()
        .unwrap_or(None)
}

/// Start a gordo-deploy job using this `Gordo`.
/// Will patch the status of the `Gordo` to reflect the current revision number
fn start_gordo_deploy_job(
    gordo: &Gordo,
    client: &APIClient,
    resource: &Api<Gordo>,
    namespace: &str,
    env_config: &GordoEnvironmentConfig,
) -> () {
    let gordo_config = serde_json::to_string(&gordo.spec.config).unwrap();

    // Create the job name.
    let job_name_suffix = format!(
        "{}-{}",
        &gordo.metadata.name,
        &gordo.metadata.generation.map(|v| v as u32).unwrap_or(0)
    );
    let job_name = deploy_job_name("gordo-dpl-", &job_name_suffix);

    // Define the owner reference info
    let owner_ref = json!([
        {"blockOwnerDeletion": true, "uid": gordo.metadata.uid, "apiVersion": "v1", "kind": "Gordo", "name": &gordo.metadata.name, "controller": true }
    ]);
    let owner_ref_as_string = serde_json::to_string(&owner_ref).unwrap();

    // TODO: Remove this after a few weeks/months when people have migrated >= 0.33 of gordo-deploy
    let gordo_deploy_key_val = if minor_version(&gordo.spec.deploy_version) >= Some(33) {
        json!({"name": "GORDO_NAME", "value": &gordo.metadata.name})
    } else {
        json!({"name": "MACHINE_CONFIG", "value": gordo_config})
    };

    // Build up the gordo-deploy environment variables
    let mut env = vec![
        gordo_deploy_key_val,
        json!({"name": "ARGO_SUBMIT", "value":  "true"}),
        json!({"name": "WORKFLOW_GENERATOR_PROJECT_NAME", "value": &gordo.metadata.name}),
        json!({"name": "WORKFLOW_GENERATOR_OWNER_REFERENCES", "value": owner_ref_as_string}),
    ];
    // push in any that were supplied by the Gordo.spec.gordo_environment mapping
    gordo.spec.deploy_environment.as_ref().map(|environment| {
        environment.iter().for_each(|(key, value)| {
            env.push(json!({"name": key, "value": value}));
        })
    });

    // Job spec for launching this gordo config into a workflow
    let spec = json!({
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {
            "name": &job_name,
            "ownerReferences": owner_ref,
            "labels": {
                "gordoProjectName": &gordo.metadata.name
            }
        },
        "spec": {
            "template": {
                "metadata": {
                    "name": &job_name
                },
                "spec": {
                    "containers": [{
                        "name": "gordo-deploy",
                        "image": &format!("{}:{}", &env_config.deploy_image, &gordo.spec.deploy_version),
                        "env": env
                    }],
                    "restartPolicy": "Never"
                }
            }
        }

    });
    let serialized_spec = serde_json::to_vec(&spec).unwrap();

    // Before launching this job, remove previous jobs for this project
    remove_gordo_deploy_jobs(&gordo, &client, &namespace);

    // Send off job, later we can add support to watching the job if needed via `jobs.watch(..)`
    info!("Launching job - {}!", &job_name);
    let postparams = PostParams::default();
    let jobs = Api::v1Job(client.clone()).within(&namespace);

    match jobs.create(&postparams, serialized_spec) {
        Ok(job) => info!("Submitted job: {:?}", job.metadata.name),
        Err(e) => {
            error!("Failed to submit job with error: {:?}", e);
            unimplemented!("Haven't implemented a way to deal with this, exiting."); // Kubernetes will restart us
        }
    }

    // Update the status of this job
    info!(
        "Setting status of this gordo '{:?}' to 'Submitted'",
        &gordo.metadata.name
    );
    let status = json!({
        "status": GordoStatus::Submitted(gordo.metadata.generation.map(|v| v as u32))
    });
    match resource.patch_status(
        &gordo.metadata.name,
        &PatchParams::default(),
        serde_json::to_vec(&status).expect("Status was not serializable, should never happen."),
    ) {
        Ok(o) => info!("Patched status: {:?}", o.status),
        Err(e) => error!("Failed to patch status: {:?}", e),
    };
}

/// Remove any gordo deploy jobs associated with this `Gordo`
pub(crate) fn remove_gordo_deploy_jobs(gordo: &Gordo, client: &APIClient, namespace: &str) -> () {
    info!(
        "Removing any gordo-deploy jobs for Gordo: '{}'",
        &gordo.metadata.name
    );

    let jobs = Api::v1Job(client.clone()).within(&namespace);
    match jobs.list(&ListParams::default()) {
        Ok(job_list) => job_list
            .items
            .iter()
            .filter(|job| job.metadata.labels.get("gordoProjectName") == Some(&gordo.metadata.name))
            .for_each(|job| {
                if let Err(err) = jobs.delete(&job.metadata.name, &DeleteParams::default()) {
                    error!(
                        "Failed to delete old gordo job: '{}' with error: {:?}",
                        &job.metadata.name, err
                    )
                }
            }),
        Err(e) => error!("Failed to list jobs: {:?}", e),
    }
}

/// Generate a name which is no greater than 63 chars in length
/// always keeping the `prefix` and as much of `suffix` as possible, favoring its ending.
pub fn deploy_job_name(prefix: &str, suffix: &str) -> String {
    let suffix = suffix
        .chars()
        .rev()
        .take(63 - prefix.len())
        .collect::<Vec<char>>()
        .iter()
        .rev()
        .collect::<String>();
    format!("{}{}", prefix, suffix)
}

use kube::api::{DeleteParams, Informer, ListParams, PatchParams};
use kube::{
    api::{Api, Object, PostParams, WatchEvent},
    client::APIClient,
    config,
};
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

type GenerationNumber = Option<u32>;
type Gordo = Object<GordoSpec, GordoStatus>;

/// Represents the 'spec' field of a Gordo resource
#[derive(Serialize, Deserialize, Clone)]
pub struct GordoSpec {
    #[serde(rename = "deploy-version")]
    deploy_version: String,
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

    let config = config::load_kube_config().unwrap_or_else(|_| {
        config::incluster_config().expect("Failed to get local kube config and incluster config")
    });
    let client = APIClient::new(config);

    let namespace = std::env::var("NAMESPACE").unwrap_or("kubeflow".into());

    let resource: Api<Gordo> = Api::customResource(client.clone(), "gordos")
        .version("v1")
        .group("equinor.com")
        .within(&namespace);

    let informer: Informer<Gordo> = Informer::new(resource.clone());

    // On start up, get a list of all gordos, and start gordo-deploy jobs for each
    // which doesn't have a Submitted(revision) which doesn't match its current revision
    // or otherwise hasn't been submitted at all.
    launch_waiting_gordo_workflows(&resource, &client, &namespace);

    loop {
        // Update state changes
        informer
            .poll()
            .unwrap_or_else(|e| error!("Failed to poll: {:?}", e));

        while let Some(event) = informer.pop() {
            match event {
                WatchEvent::Added(gordo) => {
                    start_gordo_deploy_job(&gordo, &client, &resource, &namespace)
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
                                            &gordo, &client, &resource, &namespace,
                                        );
                                    }
                                }
                            }
                        }

                        // No Gordo status
                        None => start_gordo_deploy_job(&gordo, &client, &resource, &namespace),
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
fn launch_waiting_gordo_workflows(
    resource: &Api<Gordo>,
    client: &APIClient,
    namespace: &str,
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
                        start_gordo_deploy_job(gordo, &client, &resource, &namespace)
                    })
            }
        }
        Err(e) => error!("Unable to list previous gordos: {:?}", e),
    }
}

// Get a minor version from standard SemVer string
fn minor_version(deploy_version: &str) -> Option<u32> {
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
) -> () {
    let gordo_config = serde_json::to_string(&gordo.spec.config).unwrap();

    // Create the job.
    let job_name = format!(
        "gordo-deploy-job-{}-{}",
        &gordo.metadata.name,
        &gordo.metadata.generation.map(|v| v as u32).unwrap_or(0)
    );

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
                        "image": &format!("auroradevacr.azurecr.io/gordo-infrastructure/gordo-deploy:{}", &gordo.spec.deploy_version),
                        "env": [
                            gordo_deploy_key_val,
                            {"name": "ARGO_SUBMIT", "value":  "true"},
                            {"name": "WORKFLOW_GENERATOR_PROJECT_NAME", "value": &gordo.metadata.name},
                            {"name": "WORKFLOW_GENERATOR_OWNER_REFERENCES", "value": owner_ref_as_string}
                        ]
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
fn remove_gordo_deploy_jobs(gordo: &Gordo, client: &APIClient, namespace: &str) -> () {
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

#[cfg(test)]
mod tests {

    use crate::*;

    #[test]
    fn test_minor_version() {
        assert_eq!(minor_version("0.33.0"), Some(33));
        assert_eq!(minor_version("0.31.12"), Some(31));
        assert_eq!(minor_version("0.abc.def"), None);
    }
}
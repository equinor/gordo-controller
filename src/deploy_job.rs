use crate::minor_version;
use crate::{Gordo, GordoEnvironmentConfig};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
pub struct DeployJob {
    pub name: String,
    pub manifest: Value,
}

impl DeployJob {
    /// Create a new deploy job based on a Gordo CRD and current environment.
    pub fn new(gordo: &Gordo, env_config: &GordoEnvironmentConfig) -> Self {
        // Create the job name.
        let job_name_suffix = format!(
            "{}-{}",
            &gordo.metadata.name,
            &gordo.metadata.generation.map(|v| v as u32).unwrap_or(0)
        );
        let job_name = Self::deploy_job_name("gordo-dpl-", &job_name_suffix);

        // Define the owner reference info
        let owner_references = json!([
        {"blockOwnerDeletion": true, "uid": gordo.metadata.uid, "apiVersion": "v1", "kind": "Gordo", "name": &gordo.metadata.name, "controller": true }
        ]);
        let owner_ref_as_string = serde_json::to_string(&owner_references).unwrap();

        // TODO: Remove this after a few weeks/months when people have migrated >= 0.33 of gordo-deploy
        let gordo_deploy_key_val = if minor_version(&gordo.spec.deploy_version) >= Some(33) {
            json!({"name": "GORDO_NAME", "value": &gordo.metadata.name})
        } else {
            let gordo_config = serde_json::to_string(&gordo.spec.config).unwrap();
            json!({"name": "MACHINE_CONFIG", "value": gordo_config})
        };

        // Build up the gordo-deploy environment variables
        let project_revision = chrono::Utc::now().timestamp_millis().to_string();
        let mut environment = vec![
            gordo_deploy_key_val,
            json!({"name": "ARGO_SUBMIT", "value":  "true"}),
            json!({"name": "WORKFLOW_GENERATOR_PROJECT_NAME", "value": &gordo.metadata.name}),
            json!({"name": "WORKFLOW_GENERATOR_OWNER_REFERENCES", "value": owner_ref_as_string}),
            json!({"name": "WORKFLOW_GENERATOR_PROJECT_VERSION", "value": project_revision}),
        ];

        // push in any that were supplied by the Gordo.spec.gordo_environment mapping
        gordo.spec.deploy_environment.as_ref().map(|env| {
            env.iter().for_each(|(key, value)| {
                environment.push(json!({"name": key, "value": value}));
            })
        });

        let manifest: Value = json!({
            "apiVersion": "batch/v1",
            "kind": "Job",
            "metadata": {
                "name": &job_name,
                "ownerReferences": owner_references,
                "labels": {
                    "gordoProjectName": &gordo.metadata.name
                }
            },
            "spec": {
                "ttlSecondsAfterFinished": 604800,  // 1 week in seconds
                "template": {
                    "metadata": {
                        "name": &job_name
                    },
                    "spec": {
                        "containers": [{
                            "name": "gordo-deploy",
                            "image": &format!("{}:{}", &env_config.deploy_image, &gordo.spec.deploy_version),
                            "env": environment,
                            "resources": {
                                "limits": {
                                    "memory": "1000Mi",
                                    "cpu": "2000m"
                                },
                                "requests": {
                                    "memory": "500Mi",
                                    "cpu": "250m"
                                }
                            }
                        }],
                        "restartPolicy": "Never"
                    }
                }
            }

        });
        Self {
            name: job_name,
            manifest,
        }
    }

    /// Serialize this job's udnerlying k8s manifest into a `Vec<u8>` with serde
    pub fn as_vec(&self) -> Vec<u8> {
        serde_json::to_vec(&self.manifest).unwrap()
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
}

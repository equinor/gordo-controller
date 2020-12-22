use crate::{Gordo, GordoEnvironmentConfig};
use k8s_openapi::api::core::v1::{Container, EnvVar, PodSpec, PodTemplateSpec, ResourceRequirements};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta as OpenApiObjectMeta;
use kube::api::{ObjectMeta, OwnerReference, TypeMeta};
use serde::Serialize;
use std::collections::BTreeMap;
use std::iter::FromIterator;

#[derive(Serialize, Clone)]
pub struct DeployJob {
    pub types: TypeMeta,
    pub metadata: ObjectMeta,
    pub spec: DeployJobSpec,
    pub status: Option<kube::api::Void>,
    #[serde(skip)] // This is not part of a k8s resource; for internal use.
    pub revision: String,
}

#[derive(Serialize, Clone)]
#[allow(non_snake_case)]
pub struct DeployJobSpec {
    pub ttlSecondsAfterFinished: u32,
    pub template: PodTemplateSpec,
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

        let owner_references = Self::owner_references(&gordo);
        let owner_ref_as_string = serde_json::to_string(&owner_references).unwrap();
        let project_revision = chrono::Utc::now().timestamp_millis().to_string();

        // Build up the gordo-deploy environment variables
        let mut environment: Vec<EnvVar> = vec![
            Self::env_var("GORDO_NAME", &gordo.metadata.name),
            Self::env_var("ARGO_SUBMIT", "true"),
            Self::env_var("WORKFLOW_GENERATOR_PROJECT_NAME", &gordo.metadata.name),
            Self::env_var("WORKFLOW_GENERATOR_OWNER_REFERENCES", &owner_ref_as_string),
            Self::env_var("WORKFLOW_GENERATOR_PROJECT_REVISION", &project_revision),
            // TODO: Backward compat. Until all have moved >=0.47.0 of gordo-components
            Self::env_var("WORKFLOW_GENERATOR_PROJECT_VERSION", &project_revision),
            Self::env_var("WORKFLOW_GENERATOR_DOCKER_REGISTRY", &env_config.docker_registry),
            Self::env_var("WORKFLOW_GENERATOR_GORDO_VERSION", &gordo.spec.deploy_version),
        ];

        // push in any that were supplied by the Gordo.spec.gordo_environment mapping
        gordo.spec.deploy_environment.as_ref().map(|env| {
            env.iter().for_each(|(key, value)| {
                environment.push(Self::env_var(key, value));
            })
        });

        let container = Self::container(&gordo, environment, env_config);
        let pod_spec = Self::pod_spec(vec![container]);
        let spec_metadata = Self::pod_spec_metadata(&job_name);

        Self {
            types: TypeMeta {
                apiVersion: Some("v1".to_string()),
                kind: Some("Job".to_string()),
            },
            metadata: ObjectMeta {
                name: job_name.clone(),
                namespace: None,
                creation_timestamp: None,
                deletion_timestamp: None,
                labels: Self::labels(&gordo),
                annotations: Default::default(),
                resourceVersion: None,
                ownerReferences: owner_references,
                uid: None,
                generation: None,
                generateName: None,
                initializers: None,
                finalizers: vec![],
            },
            spec: DeployJobSpec {
                ttlSecondsAfterFinished: 604800, // 1 week in seconds,
                template: PodTemplateSpec {
                    metadata: Some(spec_metadata),
                    spec: Some(pod_spec),
                },
            },
            status: None,
            revision: project_revision,
        }
    }

    fn env_var(name: &str, value: &str) -> EnvVar {
        EnvVar {
            name: name.to_string(),
            value: Some(value.to_string()),
            value_from: None,
        }
    }

    fn labels(gordo: &Gordo) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert("gordoProjectName".to_owned(), gordo.metadata.name.to_owned());
        labels
    }

    fn pod_spec_metadata(name: &str) -> OpenApiObjectMeta {
        let mut spec_metadata = OpenApiObjectMeta::default();
        spec_metadata.name = Some(name.to_string());
        spec_metadata
    }

    fn deploy_image(gordo: &Gordo, env_config: &GordoEnvironmentConfig) -> String {
        let docker_registry = match &gordo.spec.docker_registry {
            Some(docker_registry) => docker_registry,
            None => &env_config.docker_registry,
        };
        match &gordo.spec.deploy_repository {
            Some(deploy_repository) => format!("{}/{}", docker_registry, deploy_repository),
            None => {
                if !env_config.deploy_repository.is_empty() {
                    format!("{}/{}", docker_registry, env_config.deploy_repository)
                } else {
                    env_config.deploy_image.clone()
                }
            }
        }
    }

    fn container(gordo: &Gordo, environment: Vec<EnvVar>, env_config: &GordoEnvironmentConfig) -> Container {
        let mut container = Container::default();
        container.name = "gordo-deploy".to_string();
        let deploy_image = Self::deploy_image(gordo, env_config);
        container.image = Some(format!("{}:{}", deploy_image, &gordo.spec.deploy_version));
        container.command = Some(vec!["bash".to_string(), "./run_workflow_and_argo.sh".to_string()]);
        container.image_pull_policy = Some("Always".to_string());
        container.env = Some(environment);
        container.resources = Some(ResourceRequirements {
            limits: Some(BTreeMap::from_iter(
                vec![
                    ("memory".to_owned(), Quantity("1000Mi".to_owned())),
                    ("cpu".to_owned(), Quantity("2000m".to_string())),
                ]
                .into_iter(),
            )),
            requests: Some(BTreeMap::from_iter(
                vec![
                    ("memory".to_owned(), Quantity("500Mi".to_owned())),
                    ("cpu".to_owned(), Quantity("250m".to_string())),
                ]
                .into_iter(),
            )),
        });
        container
    }

    fn pod_spec(containers: Vec<Container>) -> PodSpec {
        let mut pod_spec = PodSpec::default();
        pod_spec.containers = containers;
        pod_spec.restart_policy = Some("Never".to_string());
        pod_spec
    }

    fn owner_references(gordo: &Gordo) -> Vec<OwnerReference> {
        // Define the owner reference info
        let owner_ref = OwnerReference {
            controller: Default::default(),
            blockOwnerDeletion: true,
            name: gordo.metadata.name.to_owned(),
            apiVersion: "v1".to_string(),
            kind: "Gordo".to_string(),
            uid: gordo.metadata.uid.clone().unwrap_or_default(),
        };
        vec![owner_ref]
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

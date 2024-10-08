use crate::{
    utils::{env_var, object_to_owner_reference},
    Config, Gordo,
};
use k8s_openapi::api::batch::v1::{Job, JobSpec};
use k8s_openapi::api::core::v1::{Container, EnvVar, PodSpec, PodTemplateSpec, ResourceRequirements};
use k8s_openapi::api::core::v1::{EmptyDirVolumeSource, SecurityContext, Volume, VolumeMount};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta as OpenApiObjectMeta;
use kube::api::ObjectMeta;
use log::{info, warn};
use std::collections::BTreeMap;
use std::iter::FromIterator;

// TODO builder

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

fn deploy_image(gordo: &Gordo, config: &Config) -> String {
    let docker_registry = match &gordo.spec.docker_registry {
        Some(docker_registry) => docker_registry,
        None => &config.docker_registry,
    };
    match &gordo.spec.deploy_repository {
        Some(deploy_repository) => format!("{}/{}", docker_registry, deploy_repository),
        None => {
            if !config.deploy_repository.is_empty() {
                format!("{}/{}", docker_registry, config.deploy_repository)
            } else {
                config.deploy_image.clone()
            }
        }
    }
}

fn deploy_container(gordo: &Gordo, environment: Vec<EnvVar>, config: &Config) -> Container {
    let mut container = Container::default();
    container.name = "gordo-deploy".to_string();
    let deploy_image = deploy_image(gordo, config);
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
    let mut security_context = SecurityContext::default();
    security_context.run_as_non_root = Some(true);
    if config.deploy_job_ro_fs {
        security_context.read_only_root_filesystem = Some(true);
        container.volume_mounts = Some(vec![VolumeMount {
            name: "tmp".to_string(),
            mount_path: "/tmp".to_string(),
            ..VolumeMount::default()
        }]);
    }
    container.security_context = Some(security_context);
    container
}

fn deploy_pod_spec(containers: Vec<Container>, config: &Config) -> PodSpec {
    let mut pod_spec = PodSpec::default();
    pod_spec.containers = containers;
    pod_spec.restart_policy = Some("Never".to_string());
    if config.deploy_job_ro_fs {
        pod_spec.volumes = Some(vec![Volume {
            name: "tmp".to_string(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Volume::default()
        }]);
    }
    pod_spec.service_account = config.argo_service_account.as_ref().map(|v| v.to_string());
    pod_spec
}

fn deploy_pod_spec_metadata(name: &str, resources_labels: &Option<BTreeMap<String, String>>) -> OpenApiObjectMeta {
    let mut spec_metadata = OpenApiObjectMeta::default();
    spec_metadata.name = Some(name.to_string());
    spec_metadata.labels = resources_labels.to_owned();
    spec_metadata
}

fn deploy_labels(gordo: &Gordo, resources_labels: &Option<BTreeMap<String, String>>) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    let name = match &gordo.metadata.name {
        Some(name) => name,
        None => {
            warn!("Unable to find Gordo name");
            return labels;
        }
    };
    labels.insert("gordoProjectName".to_owned(), name.to_string());
    if let Some(additional_labels) = resources_labels {
        for (label, value) in additional_labels {
            labels.insert(label.to_owned(), value.to_owned());
        }
    }
    labels
}

pub fn create_deploy_job(gordo: &Gordo, config: &Config) -> Option<Job> {
    // Create the job name.
    let name = match &gordo.metadata.name {
        Some(name) => name,
        None => {
            warn!("Gordo .metadata.name is empty");
            return None;
        }
    };
    let job_name_suffix = format!("{}-{}", name, &gordo.metadata.generation.unwrap_or(0));
    let job_name = deploy_job_name("gordo-dpl-", &job_name_suffix);

    info!("Creating job \"{}\" for Gordo \"{}\"", job_name, name);

    let owner_references_result = object_to_owner_reference::<Gordo>(gordo.metadata.clone());
    let owner_references = match owner_references_result {
        Ok(owner_references) => owner_references,
        Err(_) => {
            warn!("Unable to build owner_reference");
            return None;
        }
    };
    let owner_ref_as_string = serde_json::to_string(&vec![owner_references.clone()]).unwrap();
    let project_revision = chrono::Utc::now().timestamp_millis().to_string();
    let mut debug_show_workflow = "";
    if gordo.spec.debug_show_workflow.unwrap_or(false) {
        debug_show_workflow = "true"
    }

    // TODO Handle possible panic here
    let resources_labels = config.get_resources_labels_json().unwrap();

    let mut initial_environment: BTreeMap<String, String> = BTreeMap::new();

    for (key, value) in config.workflow_generator_envs.iter() {
        initial_environment.insert(key.into(), value.into());
    }

    // Build up the gordo-deploy environment variables
    initial_environment.insert("GORDO_NAME".into(), name.into());
    initial_environment.insert("ARGO_SUBMIT".into(), "true".into());
    initial_environment.insert("WORKFLOW_GENERATOR_PROJECT_NAME".into(), name.clone());
    initial_environment.insert("WORKFLOW_GENERATOR_OWNER_REFERENCES".into(), owner_ref_as_string);
    initial_environment.insert("WORKFLOW_GENERATOR_PROJECT_REVISION".into(), project_revision.clone());
    // TODO: Backward compat. Until all have moved >=0.47.0 of gordo-components
    initial_environment.insert("WORKFLOW_GENERATOR_PROJECT_VERSION".into(), project_revision);
    initial_environment.insert(
        "WORKFLOW_GENERATOR_DOCKER_REGISTRY".into(),
        config.docker_registry.clone(),
    );
    initial_environment.insert(
        "WORKFLOW_GENERATOR_GORDO_VERSION".into(),
        gordo.spec.deploy_version.clone(),
    );
    initial_environment.insert("WORKFLOW_GENERATOR_RESOURCE_LABELS".into(), resources_labels);
    initial_environment.insert("DEBUG_SHOW_WORKFLOW".into(), debug_show_workflow.into());

    // As long as we calling env_config.validate() method in the main function
    // there should not be circumstances from which panic should occur here
    let default_deploy_environment = &config.default_deploy_environment;

    if let Some(deploy_environment) = default_deploy_environment {
        for (key, value) in deploy_environment.iter() {
            initial_environment.insert(key.into(), value.into());
        }
    }

    if let Some(argo_service_account) = &config.argo_service_account {
        initial_environment.insert("ARGO_SERVICE_ACCOUNT".into(), argo_service_account.into());
    }

    initial_environment.insert(
        "ARGO_VERSION_NUMBER".into(),
        config.argo_version_number.map_or("".into(), |v| v.to_string()),
    );

    let resources_labels = &config.resources_labels;

    // push in any that were supplied by the Gordo.spec.gordo_environment mapping
    gordo.spec.deploy_environment.as_ref().map(|env| {
        env.iter().for_each(|(key, value)| {
            initial_environment.insert(key.into(), value.into());
        })
    });

    let mut environment: Vec<EnvVar> = vec![];
    initial_environment.iter().for_each(|(key, value)| {
        environment.push(env_var(key, value));
    });

    let container = deploy_container(&gordo, environment, config);
    let pod_spec = deploy_pod_spec(vec![container], config);
    let spec_metadata = deploy_pod_spec_metadata(&job_name, resources_labels);

    let mut metadata = ObjectMeta::default();
    metadata.name = Some(job_name.clone());
    metadata.labels = Some(deploy_labels(&gordo, resources_labels));
    metadata.annotations = Default::default();
    metadata.owner_references = Some(vec![owner_references.clone()]);
    metadata.finalizers = Some(vec![]);

    let mut job_spec = JobSpec::default();
    job_spec.ttl_seconds_after_finished = Some(604800); // 1 week in seconds
    job_spec.template = PodTemplateSpec {
        metadata: Some(spec_metadata),
        spec: Some(pod_spec),
    };

    Some(Job {
        metadata,
        spec: Some(job_spec),
        status: None,
    })
}

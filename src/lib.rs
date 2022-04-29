use std::result::Result;
use std::sync::Arc;
use serde::Deserialize;
use serde_json;
use futures::StreamExt;
use kube::{
    api::{Api, ListParams},
    client::Client,
};
use kube::runtime::controller::{Context, Controller, Action};
use k8s_openapi::{
    api::core::v1::Pod,
};
use log::{info, warn, debug};
use tokio::time::Duration;
use crate::crd::metrics::{RECONCILE_GORDO_COUNT, RECONCILE_GORDO_SUCCEDED, RECONCILE_GORDO_ERROR};

pub mod crd;
pub mod deploy_job;
pub mod views;
pub mod utils;
pub mod errors;

use crate::crd::{
    gordo::{Gordo, handle_gordo_state},
    model::{monitor_models, Model},
    pod::{monitor_pods},
    argo::{monitor_wf, Workflow},
};
pub use deploy_job::create_deploy_job;
use std::collections::{HashMap, BTreeMap};
use errors::Error;

fn default_deploy_repository() -> String {
    "".to_string()
}

fn default_server_port() -> u16 {
    8888
}

fn default_server_host() -> String {
    String::from("0.0.0.0")
}

fn default_deploy_ro_fs() -> bool {
    false
}

#[derive(Deserialize, Debug, Clone)]
pub struct GordoEnvironmentConfig {
    pub deploy_image: String,
    #[serde(default="default_deploy_repository")]
    pub deploy_repository: String,
    #[serde(default="default_server_port")]
    pub server_port: u16,
    #[serde(default="default_server_host")]
    pub server_host: String,
    pub docker_registry: String,
    pub default_deploy_environment: String,
    pub resources_labels: String,
    #[serde(default="default_deploy_ro_fs")]
    pub deploy_job_ro_fs: bool,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub deploy_image: String,
    pub deploy_repository: String,
    pub server_port: u16,
    pub server_host: String,
    pub docker_registry: String,
    pub default_deploy_environment: Option<HashMap<String, String>>,
    pub resources_labels: Option<BTreeMap<String, String>>,
    pub deploy_job_ro_fs: bool,
}

impl Config {

    pub fn from_env_config(env_config: GordoEnvironmentConfig) -> Result<Self, String> {
        let default_deploy_environment: Option<HashMap<String, String>> = Config::load_from_json(&env_config.default_deploy_environment)?;
        let resources_labels: Option<BTreeMap<String, String>> = Config::load_from_json(&env_config.resources_labels)?;
        Ok(Config {
            deploy_image: env_config.deploy_image.clone(),
            deploy_repository: env_config.deploy_repository.clone(),
            server_port: env_config.server_port,
            server_host: env_config.server_host.clone(),
            docker_registry: env_config.docker_registry.clone(),
            deploy_job_ro_fs: env_config.deploy_job_ro_fs,
            default_deploy_environment,
            resources_labels,
        })
    }

    pub fn load_from_json<'a, T>(json_value: &'a str) -> Result<Option<T>, String> where T: Deserialize<'a> {
        if json_value.is_empty() {
            return Ok(None);
        }
        let result: Result<T, _> = serde_json::from_str(json_value);
        match result {
            Ok(value) => Ok(Some(value)),
            Err(err) => Err(err.to_string()),
        }
    }

    pub fn get_resources_labels_json(&self) -> Result<String, String> {
        if let Some(resources_labels) = &self.resources_labels {
            return match serde_json::to_string(resources_labels) {
                Ok(value) => Ok(value),
                Err(err) => Err(err.to_string()),
            }
        }
        Ok("".to_string())
    }
}

impl Default for GordoEnvironmentConfig {
    fn default() -> Self {
        GordoEnvironmentConfig {
            deploy_image: "gordo-infrastructure/gordo-deploy".to_owned(),
            deploy_repository: "".to_owned(),
            server_port: 8888,
            server_host: "0.0.0.0".to_owned(),
            docker_registry: "docker.io".to_owned(),
            default_deploy_environment: "".to_owned(),
            resources_labels: "".to_owned(),
            deploy_job_ro_fs: false,
        }
    }
}

struct Data {
    client: Client,
    config: Config,
}


#[warn(unused_variables)]
async fn reconcile_gordo(gordo: Arc<Gordo>, ctx: Context<Data>) -> Result<Action, Error> {
    let namespace = gordo
        .metadata
        .namespace
        .as_ref()
        .ok_or(Error::MissingKey(".metadata.namespace"))?;
    let gordo_name = gordo.metadata.name.as_ref().ok_or(Error::MissingKey(".metadata.name"))?;

    RECONCILE_GORDO_COUNT.with_label_values(&[&gordo_name]).inc();

    let client = ctx.get_ref().client.clone();
    let config = ctx.get_ref().config.clone();

    let gordo_api: Api<Gordo> = Api::namespaced(client.clone(), namespace);
    info!("Reconcile gordo: {:?}, namespace: {:?}", gordo_name, namespace);
    let model_labels = format!("applications.gordo.equinor.com/project-name={}", gordo_name);
    let lp = ListParams::default().labels(&model_labels);

    handle_gordo_state(&gordo, &client, &gordo_api, namespace, &config).await.map_err(|e| Error::KubeError(e))?;

    let model_api: Api<Model> = Api::namespaced(client.clone(), namespace);
    let models_obj_list = model_api.list(&lp).await.map_err(Error::KubeError)?;
    let models: Vec<_> = models_obj_list.into_iter().collect();
    let names = utils::resource_names(&models);
    debug!("Reconcile {} {}: {}", models.len(), utils::plural_str(models.len(), "models"), names);
    // TODO deal with Arc here in right way
    monitor_models(&model_api, &gordo_api, &models, &vec![(*gordo).clone()]).await;

    let workflow_api: Api<Workflow> = Api::namespaced(client.clone(), namespace);
    let workflows_obj_list = workflow_api.list(&lp).await.map_err(Error::KubeError)?;
    let workflows: Vec<_> = workflows_obj_list.into_iter().collect();
    let names = utils::resource_names(&workflows);
    debug!("Reconcile {} {}: {}", workflows.len(), utils::plural_str(workflows.len(), "workflows"), names);

    let pod_api: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let pod_obj_list = pod_api.list(&lp).await.map_err(Error::KubeError)?;
    let pods: Vec<_> = pod_obj_list.into_iter().collect();
    let names = utils::resource_names(&pods);
    debug!("Reconcile {} {}: {}", pods.len(), utils::plural_str(pods.len(), "pods"), names);

    monitor_wf(&model_api, &workflows, &models, &pods).await;
    monitor_pods(&model_api, &models, &pods).await;

    Ok(Action::requeue(Duration::from_secs(300)))
}


fn error_policy(_error: &Error, _ctx: Context<Data>) -> Action {
    Action::requeue(Duration::from_secs(30))
}

pub async fn init_gordo_controller(client: Client, config: Config) {
    let gordo: Api<Gordo> = Api::default_namespaced(client.clone());
    let model: Api<Pod> = Api::default_namespaced(client.clone());
    let workflow: Api<Workflow> = Api::default_namespaced(client.clone());

    log::info!("Starting gordo-controller");

    Controller::new(gordo, ListParams::default())
        .owns(model, ListParams::default())
        .owns(workflow, ListParams::default())
        .shutdown_on_signal()
        .run(reconcile_gordo, error_policy, Context::new(Data { client, config }))
        .for_each(|res| async move {
            match res {
                Ok(o) => {
                    info!("Reconciled {:?}", o);
                    RECONCILE_GORDO_SUCCEDED.with_label_values(&[]).inc();
                }
                Err(e) => {
                    warn!("Reconcile failed: {:?}", e);
                    RECONCILE_GORDO_ERROR.with_label_values(&[]).inc();
                },
            }
        }).await;
}

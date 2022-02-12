use futures::future::join4;
use std::result::{Result};
use kube::config;
use kube::{api::Object, config::Configuration};
use k8s_openapi::api::core::v1::{PodSpec, PodStatus};
use log::error;
use serde::Deserialize;
use serde_json;
use futures::StreamExt;
use kube::{
    api::{Api, ListParams, Resource},
    client::Client,
    CustomResource,
};
use kube_runtime::controller::{Context, Controller, ReconcilerAction};
use k8s_openapi::{
    api::core::v1::Pod,
    apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference},
};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{Value};
use tokio::time::Duration;
use schemars::JsonSchema;
use thiserror::Error;

pub mod crd;
pub mod deploy_job;
pub mod views;
pub mod utils;
pub mod errors;

use crate::crd::{
    gordo::{Gordo},
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
        }
    }
}

#[warn(unused_variables)]
async fn reconcile(gordo: Gordo, ctx: Context<Data>) -> Result<ReconcilerAction, Error> {
    info!("reconcile gordo {:?}", gordo);
    let namespace = gordo
        .metadata
        .namespace
        .as_ref()
        .ok_or(Error::MissingKey(".metadata.namespace"))?;
    info!("namespace {:?}", namespace);

    let client = ctx.get_ref().client.clone();
    let gordo_name = gordo.metadata.name.as_ref().ok_or(Error::MissingKey(".metadata.name"))?;
    let model_labels = format!("applications.gordo.equinor.com/project-name={}", gordo_name);
    let lp = ListParams::default().labels(&model_labels);

    let model_api: Api<Model> = Api::namespaced(client.clone(), namespace);
    let models_obj_list = model_api.list(&lp).await.map_err(Error::KubeError)?;
    let models: Vec<_> = models_obj_list.iter().collect();
    info!("models {:?}", models);

    let workflow_api: Api<Workflow> = Api::namespaced(client.clone(), namespace);
    let workflows_obj_list = workflow_api.list(&lp).await.map_err(Error::KubeError)?;
    let workflows: Vec<_> = workflows_obj_list.iter().collect();
    info!("workflows {:?}", workflows);

    Ok(ReconcilerAction {
        requeue_after: Some(Duration::from_secs(300)),
    })
}


fn error_policy(_error: &Error, _ctx: Context<Data>) -> ReconcilerAction {
    ReconcilerAction {
        requeue_after: Some(Duration::from_secs(60)),
    }
}

struct Data {
    client: Client,
}

#[actix_rt::main]
async fn main() -> Result<(), kube::Error> {
    //TODO do not forget about RUST_LOG env in all deployment scripts
    env_logger::init();

    let client = Client::try_default().await?;

    let gordo: Api<Gordo> = Api::default_namespaced(client.clone());
    let model: Api<Pod> = Api::default_namespaced(client.clone());
    let workflow: Api<Workflow> = Api::default_namespaced(client.clone());

    log::info!("starting gordo-controller");

    Controller::new(gordo, ListParams::default())
        .owns(model, ListParams::default())
        .owns(workflow, ListParams::default())
        .shutdown_on_signal()
        .run(reconcile, error_policy, Context::new(Data { client }))
        .for_each(|res| async move {
            match res {
                Ok(o) => info!("reconciled {:?}", o),
                Err(e) => warn!("reconcile failed: {}", e),
            }
        })
        .await;
    log::info!("controller terminated");
    Ok(())
}

#[derive(Clone)]
pub struct Manager {
    client: APIClient,
    namespace: String,
    gordo_resource: Api<Gordo>,
    model_rf: Reflector<Model>,
    model_resource: Api<Model>,
    pod_rf: Reflector<Object<PodSpec, PodStatus>>,
    pod_resource: Api<Object<PodSpec, PodStatus>>,
    wf_rf: Reflector<ArgoWorkflow>,
    wf_resource: Api<ArgoWorkflow>,
    config: Config,
}

impl Manager {
    /// Create a new instance of the Gordo Controller
    pub async fn new(kube_config: Configuration, config: Config) -> Self {
        let timeout = 15;

        let namespace = kube_config.default_ns.to_owned();
        let client = APIClient::new(kube_config);

        let model_resource = load_model_resource(&client, &namespace);
        let model_rf = Reflector::new(model_resource.clone()).timeout(timeout).init().await.unwrap();

        let gordo_resource = load_gordo_resource(&client, &namespace);
        let gordo_rf = Reflector::new(gordo_resource.clone()).timeout(timeout).init().await.unwrap();

        let pod_resource = Api::v1Pod(client.clone()).within(&namespace);
        let pod_rf = Reflector::new(pod_resource.clone()).timeout(timeout).labels("app==gordo-model-builder").init().await.unwrap();

        let wf_resource = load_argo_workflow_resource(&client, &namespace);
        let wf_rf = Reflector::new(wf_resource.clone()).timeout(timeout).init().await.unwrap();

        Controller {
            client,
            namespace,
            gordo_rf,
            gordo_resource,
            model_rf,
            model_resource,
            pod_rf,
            pod_resource,
            wf_rf,
            wf_resource,
            config,
        }
    }

    /// Poll the Gordo and Model reflectors
    async fn poll(&self) -> Result<(), kube::Error> {
        // Poll both reflectors for Models and Gordos
        let (result1, result2, result3, result4) = join4(self.gordo_rf.poll(), self.model_rf.poll(), self.pod_rf.poll(), self.wf_rf.poll()).await;

        // Make changes based on the current state
        join4(monitor_gordos(&self), monitor_models(&self), monitor_pods(&self), monitor_wf(&self)).await;

        // Return any error, or return Ok
        result1?;
        result2?;
        result3?;
        result4?;
        Ok(())
    }

    /// Current state of Gordos
    pub async fn gordo_state(&self) -> Vec<Gordo> {
        self.gordo_rf.state().await.unwrap_or_default()
    }
    /// Current state of Models
    pub async fn model_state(&self) -> Vec<Model> {
        self.model_rf.state().await.unwrap_or_default()
    }
    pub async fn wf_state(&self) -> Vec<ArgoWorkflow> {
        self.wf_rf.state().await.unwrap_or_default()
    }
    /// Current state of Pods
    pub async fn pod_state(&self) -> Vec<Object<PodSpec, PodStatus>> {
        self.pod_rf.state().await.unwrap_or_default()
    }
}

pub async fn controller_init() {
    let controller = Controller::new(kube_config, config).await;

    // Continuously poll `Controller::poll` to keep the app state current
    let c1 = controller.clone();
    tokio::spawn(async move {
        loop {
            if let Err(err) = c1.poll().await {
                error!("Controller polling encountered an error: {:?}", err);
                crd::metrics::KUBE_ERRORS.with_label_values(&["controller_polling", "unknown"]).inc_by(1);
            }
        }
    });
    Ok(controller)
}

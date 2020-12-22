use futures::future::join4;
use kube::config;
use kube::{api::Reflector, api::Object, client::APIClient, config::Configuration};
use k8s_openapi::api::core::v1::{PodSpec, PodStatus};
use log::error;
use serde::Deserialize;

pub mod crd;
pub mod deploy_job;
pub mod views;

use crate::crd::{
    gordo::{load_gordo_resource, monitor_gordos, Gordo},
    model::{load_model_resource, monitor_models, Model},
    pod::{monitor_pods},
    argo::{load_argo_workflow_resource, monitor_wf, ArgoWorkflow},
};
pub use deploy_job::DeployJob;
use kube::api::Api;

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
}
impl Default for GordoEnvironmentConfig {
    fn default() -> Self {
        GordoEnvironmentConfig {
            deploy_image: "gordo-infrastructure/gordo-deploy".to_owned(),
            deploy_repository: "".to_owned(),
            server_port: 8888,
            server_host: "0.0.0.0".to_owned(),
            docker_registry: "docker.io".to_owned(),
        }
    }
}


/// Load the `kube::Configuration` giving priority to local, falling back to in-cluster config
pub async fn load_kube_config() -> Configuration {
    config::load_kube_config()
        .await
        .unwrap_or_else(|_| config::incluster_config().expect("Failed to get local kube config and incluster config"))
}

#[derive(Clone)]
pub struct Controller {
    client: APIClient,
    namespace: String,
    gordo_rf: Reflector<Gordo>,
    gordo_resource: Api<Gordo>,
    model_rf: Reflector<Model>,
    model_resource: Api<Model>,
    pod_rf: Reflector<Object<PodSpec, PodStatus>>,
    pod_resource: Api<Object<PodSpec, PodStatus>>,
    wf_rf: Reflector<ArgoWorkflow>,
    wf_resource: Api<ArgoWorkflow>,
    env_config: GordoEnvironmentConfig,
}

impl Controller {
    /// Create a new instance of the Gordo Controller
    pub async fn new(kube_config: Configuration, env_config: GordoEnvironmentConfig) -> Self {
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
            env_config,
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

/// This returns a `Controller` and calls `poll` on it continuously.
/// While at the same time initializing the monitoring of `Gorod`s and `Model`s
pub async fn controller_init(
    kube_config: Configuration,
    env_config: GordoEnvironmentConfig,
) -> Result<Controller, kube::Error> {
    let controller = Controller::new(kube_config, env_config).await;

    // Continuously poll `Controller::poll` to keep the app state current
    let c1 = controller.clone();
    tokio::spawn(async move {
        loop {
            if let Err(err) = c1.poll().await {
                error!("Controller polling encountered an error: {:?}", err);
            }
        }
    });
    Ok(controller)
}

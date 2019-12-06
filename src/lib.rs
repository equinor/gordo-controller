use futures::future::join;
use kube::config;
use kube::{api::Reflector, client::APIClient, config::Configuration};
use log::error;
use serde::Deserialize;

pub mod crd;
pub mod deploy_job;
pub mod views;

use crate::crd::{
    gordo::{load_gordo_resource, monitor_gordos, Gordo},
    model::{load_model_resource, monitor_models, Model},
};
pub use deploy_job::DeployJob;
use kube::api::Api;

#[derive(Deserialize, Debug, Clone)]
pub struct GordoEnvironmentConfig {
    pub deploy_image: String,
    pub server_port: u16,
    pub server_host: String,
}
impl Default for GordoEnvironmentConfig {
    fn default() -> Self {
        GordoEnvironmentConfig {
            deploy_image: "auroradevacr.azurecr.io/gordo-infrastructure/gordo-deploy".to_owned(),
            server_port: 8888,
            server_host: "0.0.0.0".to_owned(),
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
    env_config: GordoEnvironmentConfig,
}

impl Controller {
    /// Create a new instance of the Gordo Controller
    pub async fn new(kube_config: Configuration, env_config: GordoEnvironmentConfig) -> Self {
        let namespace = kube_config.default_ns.to_owned();
        let client = APIClient::new(kube_config);

        let model_resource = load_model_resource(&client, &namespace);
        let model_rf = Reflector::new(model_resource.clone()).timeout(15).init().await.unwrap();

        let gordo_resource = load_gordo_resource(&client, &namespace);
        let gordo_rf = Reflector::new(gordo_resource.clone()).timeout(15).init().await.unwrap();

        Controller {
            client,
            namespace,
            gordo_rf,
            gordo_resource,
            model_rf,
            model_resource,
            env_config,
        }
    }

    /// Poll the Gordo and Model reflectors
    async fn poll(&self) -> Result<(), kube::Error> {
        // Poll both reflectors for Models and Gordos
        let (result1, result2) = join(self.gordo_rf.poll(), self.model_rf.poll()).await;

        // Make changes based on the current state
        join(monitor_gordos(&self), monitor_models(&self)).await;

        // Return any error, or return Ok
        result1?;
        result2?;
        Ok(())
    }

    /// Current state of Gordos
    pub async fn gordo_state(&self) -> Vec<Gordo> {
        self.gordo_rf.read().unwrap_or_default()
    }
    /// Current state of Models
    pub async fn model_state(&self) -> Vec<Model> {
        self.model_rf.read().unwrap_or_default()
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

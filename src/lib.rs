use futures::future::join;
use kube::config;
use kube::{api::Reflector, client::APIClient, config::Configuration};
use log::error;
use serde::Deserialize;

pub mod crd;
pub mod deploy_job;
pub mod views;

use crate::crd::{
    gordo::{load_gordo_resource, monitor_gordos},
    model::{load_model_resource, monitor_models, Model},
};
pub use crd::gordo::Gordo;
pub use deploy_job::DeployJob;

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

// Get a minor version from standard SemVer string
pub fn minor_version(deploy_version: &str) -> Option<u32> {
    deploy_version
        .split('.')
        .nth(1)
        .map(|v| v.parse::<u32>().ok())
        .unwrap_or(None)
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
    model_rf: Reflector<Model>,
}

impl Controller {
    /// Create a new instance of the Gordo Controller
    pub async fn new(kube_config: Configuration) -> Self {
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
            model_rf,
        }
    }

    /// Poll the Gordo and Model reflectors
    async fn poll(&self) -> Result<(), kube::Error> {
        let (result1, result2) = join(self.gordo_rf.poll(), self.model_rf.poll()).await;

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
    let controller = Controller::new(kube_config).await;

    // Continuously poll `Controller::poll` to keep the app state current
    let c1 = controller.clone();
    tokio::spawn(async move {
        loop {
            if let Err(err) = c1.poll().await {
                error!("Failed polling Controller with error: {:?}", err);
            };
        }
    });

    // Start the normal monitoring of Gordos and Models to direct desired state changes
    let c2 = controller.clone();
    tokio::spawn(async move {
        join(
            monitor_gordos(&c2.client, &c2.namespace, &env_config),
            monitor_models(&c2.client, &c2.namespace, &env_config),
        )
        .await;
    });
    Ok(controller)
}

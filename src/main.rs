use futures::future::join;
use kube::{client::APIClient, config};
use log::error;
use serde::Deserialize;

mod crd;
mod deploy_job;
#[cfg(test)]
mod tests;

use crate::crd::gordo::Gordo;
use crate::deploy_job::DeployJob;

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

#[tokio::main]
async fn main() -> () {
    std::env::set_var("RUST_LOG", "info,kube=info");
    env_logger::init();

    let env_config = envy::from_env::<GordoEnvironmentConfig>().unwrap_or_else(|e| {
        error!("Failed to load environment config, using defaults: {:?}", e);
        GordoEnvironmentConfig::default()
    });

    let kube_config = config::load_kube_config()
        .await
        .unwrap_or_else(|_| config::incluster_config().expect("Failed to get local kube config and incluster config"));

    let namespace = kube_config.default_ns.to_owned();
    let client = APIClient::new(kube_config);

    join(
        crate::crd::gordo::monitor_gordos(&client, &namespace, &env_config),
        crate::crd::model::monitor_models(&client, &namespace, &env_config),
    )
    .await;
}

// Get a minor version from standard SemVer string
pub fn minor_version(deploy_version: &str) -> Option<u32> {
    deploy_version
        .split('.')
        .nth(1)
        .map(|v| v.parse::<u32>().ok())
        .unwrap_or(None)
}

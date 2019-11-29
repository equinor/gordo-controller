use futures::future::join;
use kube::{client::APIClient, config};
use log::info;

use gordo_controller::GordoEnvironmentConfig;

#[tokio::main]
async fn main() -> () {
    std::env::set_var("RUST_LOG", "info,kube=info");
    env_logger::init();

    let env_config = envy::from_env::<GordoEnvironmentConfig>().unwrap_or_default();
    info!("Starting with environment config: {:?}", &env_config);

    let kube_config = config::load_kube_config()
        .await
        .unwrap_or_else(|_| config::incluster_config().expect("Failed to get local kube config and incluster config"));

    let namespace = kube_config.default_ns.to_owned();
    let client = APIClient::new(kube_config);

    join(
        gordo_controller::crd::gordo::monitor_gordos(&client, &namespace, &env_config),
        gordo_controller::crd::model::monitor_models(&client, &namespace, &env_config),
    )
    .await;
}

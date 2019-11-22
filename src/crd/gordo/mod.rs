use kube::{
    api::{Api, Informer, WatchEvent},
    client::APIClient,
};
use log::{error, info};

use crate::GordoEnvironmentConfig;

pub mod gordo;
pub use gordo::*;

pub async fn monitor_gordos(client: &APIClient, namespace: &str, env_config: &GordoEnvironmentConfig) -> ! {
    let gordo_resource: Api<Gordo> = Api::customResource(client.clone(), "gordos")
        .version("v1")
        .group("equinor.com")
        .within(&namespace);
    let gordo_informer: Informer<Gordo> = Informer::new(gordo_resource.clone()).init().await.unwrap();

    // On start up, get a list of all gordos, and start gordo-deploy jobs for each
    // which doesn't have a Submitted(revision) which doesn't match its current revision
    // or otherwise hasn't been submitted at all.
    crate::crd::gordo::launch_waiting_gordo_workflows(&gordo_resource, &client, &namespace, &env_config).await;

    let mut outdated_version = false;

    loop {
        // Update state changes
        gordo_informer
            .poll()
            .await
            .unwrap_or_else(|e| panic!("Failed to poll: {:?}", e));

        while let Some(event) = gordo_informer.pop() {
            if let Err(err) = handle_gordo_event(event, &client, &gordo_resource, &namespace, &env_config).await {
                error!("Watch event error for gordo informer: {:?}", err);
                outdated_version = true;
            }
        }

        // Reset the informer if an error was encountred.
        if outdated_version {
            gordo_informer.reset().await.unwrap();
            outdated_version = false;
        }
    }
}

async fn handle_gordo_event(
    event: WatchEvent<Gordo>,
    client: &APIClient,
    resource: &Api<Gordo>,
    namespace: &str,
    env_config: &GordoEnvironmentConfig,
) -> Result<(), kube::ApiError> {
    match event {
        WatchEvent::Added(gordo) => {
            crate::crd::gordo::start_gordo_deploy_job(&gordo, &client, &resource, &namespace, &env_config).await;
        }
        WatchEvent::Modified(gordo) => {
            info!(
                "Gordo resource modified: {:?}, status is: {:?}",
                &gordo.metadata.name, &gordo.status
            );
            match gordo.status {
                Some(ref status) => {
                    match status {
                        GordoStatus::Submitted(ref generation) => {
                            // If it's submitted, we only want to launch the job if the GenerationNumber has changed.
                            if generation != &gordo.metadata.generation.map(|v| v as u32) {
                                crate::crd::gordo::start_gordo_deploy_job(
                                    &gordo,
                                    &client,
                                    &resource,
                                    &namespace,
                                    &env_config,
                                )
                                .await;
                            }
                        }
                    }
                }

                // No Gordo status
                None => {
                    crate::crd::gordo::start_gordo_deploy_job(&gordo, &client, &resource, &namespace, &env_config)
                        .await;
                }
            }
        }
        WatchEvent::Deleted(gordo) => {
            info!("Gordo resource deleted: {:?}", gordo.metadata.name);

            // Remove any old jobs associated with this Gordo which has been deleted.
            crate::crd::gordo::remove_gordo_deploy_jobs(&gordo, &client, &namespace).await;
        }
        WatchEvent::Error(err) => return Err(err),
    }
    Ok(())
}

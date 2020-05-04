use futures::future::join_all;
use kube::{api::Api, client::APIClient};
use log::{error, info};

use crate::{Controller, GordoEnvironmentConfig};

pub mod gordo;
pub use gordo::*;

pub async fn monitor_gordos(controller: &Controller) -> () {
    let gordos = controller.gordo_state().await;

    let results = join_all(gordos.iter().map(|gordo| {
        handle_gordo_state(
            gordo,
            &controller.client,
            &controller.gordo_resource,
            &controller.namespace,
            &controller.env_config,
        )
    }))
    .await;

    // Log any errors in handling state
    results.iter().for_each(|result| {
        if let Err(err) = result {
            error!("{:?}", err);
        }
    });
}

async fn handle_gordo_state(
    gordo: &Gordo,
    client: &APIClient,
    resource: &Api<Gordo>,
    namespace: &str,
    env_config: &GordoEnvironmentConfig,
) -> Result<(), kube::Error> {
    let should_start_deploy_job = match gordo.status {
        Some(ref status) => {
            match status.submission_status {
                GordoSubmissionStatus::Submitted(ref generation) => {
                    // If it's submitted, we only want to launch the job if the GenerationNumber has changed.
                    generation != &gordo.metadata.generation.map(|v| v as u32)
                }
            }
        }

        // Gordo doesn't have a status, so it must need starting
        None => true,
    };

    if should_start_deploy_job {
        let start_job_result = crate::crd::gordo::start_gordo_deploy_job(&gordo, &client, &namespace, &env_config).await;
        match start_job_result {
            Ok(job) => {
                info!("Submitted job: {:?}", job.metadata.name);
                let mut status = GordoStatus::from(gordo);
                status.project_revision = job.revision.to_owned();
        
                patch_gordo_status(gordo, status, resource).await;
            }
            Err(e) => error!("Failed to submit job with error: {:?}", e),
        }
    }

    Ok(())
}

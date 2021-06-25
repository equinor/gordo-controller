use futures::future::join_all;
use kube::{api::Api, client::APIClient};
use log::error;
use std::collections::{HashSet};

use crate::{Controller, GordoEnvironmentConfig};
use crate::crd::metrics::{KUBE_ERRORS, update_gordo_projects, GORDO_PULLING};

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

    let gordo_projects: HashSet<String> = gordos.into_iter()
        .map(|gordo| { gordo.metadata.name })
        .collect();

    // Log any errors in handling state
    results.iter().for_each(|result| {
        if let Err(err) = result {
            error!("{:?}", err);
            KUBE_ERRORS.with_label_values(&["monitor_gordos", "unknown"]).inc_by(1);
        }
    });

    update_gordo_projects(&gordo_projects);

    GORDO_PULLING.with_label_values(&[]).inc();
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
        crate::crd::gordo::start_gordo_deploy_job(&gordo, &client, &resource, &namespace, &env_config).await;
    }
    Ok(())
}

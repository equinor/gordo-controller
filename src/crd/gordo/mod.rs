use log::{error, info};

use crate::Controller;
use crate::crd::gordo::start_gordo_deploy_job;

pub mod gordo;
pub use gordo::*;

pub async fn monitor_gordos(controller: &Controller) -> () {
    let gordos = controller.gordo_state().await;

    for gordo in gordos {
        let orig_status = GordoStatus::from(&gordo);
        let mut new_status = orig_status.clone();
    
        if should_start_deploy_job(&gordo) {
            let start_job_result = start_gordo_deploy_job(&gordo, &controller.client, &controller.namespace, &controller.env_config).await;
            match start_job_result {
                Ok(job) => {
                    info!("Submitted job: {:?}", job.metadata.name);
                    new_status.project_revision = job.revision.to_owned();
                }
                Err(e) => error!("Failed to submit job with error: {:?}", e),
            }
        }
    
        if new_status != orig_status {
            patch_gordo_status(&gordo, new_status, &controller.gordo_resource).await;
        }
    }
}

fn should_start_deploy_job(gordo: &Gordo) -> bool {
    match gordo.status {
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
    }
}

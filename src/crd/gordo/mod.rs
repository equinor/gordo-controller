use kube::{api::Api, client::Client};

use crate::Config;

pub mod gordo;
pub use gordo::{start_gordo_deploy_job, Gordo, GordoSubmissionStatus};

pub async fn handle_gordo_state(
    gordo: &Gordo,
    client: &Client,
    resource: &Api<Gordo>,
    namespace: &str,
    config: &Config,
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
        crate::crd::gordo::start_gordo_deploy_job(&gordo, &client, &resource, &namespace, &config).await;
    }
    Ok(())
}

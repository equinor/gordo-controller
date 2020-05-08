use log::{error, info, warn, debug};
use futures::future::join_all;

use crate::Controller;
use crate::crd::gordo::start_gordo_deploy_job;
use crate::crd::argo::{ArgoWorkflow, ArgoWorkflowPhase};
use crate::crd::model::{Model, ModelStatus, ModelPhase, ModelPodTerminatedStatus, filter_models_on_gordo, patch_model_status};
use crate::crd::pod::POD_MATCH_LABELS;

use k8s_openapi::api::core::v1::ContainerStateTerminated;
use chrono::MIN_DATE;
use serde_json::Value;

pub mod gordo;
pub use gordo::*;

const WF_MATCH_LABELS: &'static [&'static str] = &[
    "applications.gordo.equinor.com/project-name", 
    "applications.gordo.equinor.com/project-revision", 
];

pub fn some_of_workflows_in_phases(workflows: &Vec<&ArgoWorkflow>, phases: Vec<ArgoWorkflowPhase>) -> bool {
    workflows.iter()
        .any(|wf| match &wf.status {
            Some(status) => match &status.phase {
                Some(status_phase) => (&phases).into_iter().find(|phase| &status_phase == phase).is_some(),
                None => false,
            },
            _ => false,
        })
}

pub fn all_of_workflows_in_phases(workflows: &Vec<&ArgoWorkflow>, phases: Vec<ArgoWorkflowPhase>) -> bool {
    workflows.iter()
        .all(|wf| match &wf.status {
            Some(status) => match &status.phase {
                Some(status_phase) => (&phases).into_iter().find(|phase| &status_phase == phase).is_some(),
                None => false,
            },
            _ => false,
        })
}

pub async fn monitor_gordos(controller: &Controller) -> () {
    let gordos = controller.gordo_state().await;
    let workflows = controller.wf_state().await;

    for gordo in gordos {
        let orig_status = GordoStatus::from(&gordo);
        let mut new_status = orig_status.clone();
    
        if should_start_deploy_job(&gordo) {
            info!("Starting deploy gordo '{}'", gordo.metadata.name);
            let start_job_result = start_gordo_deploy_job(&gordo, &controller.client, &controller.namespace, &controller.env_config).await;
            match start_job_result {
                Ok(job) => {
                    info!("Submitted job: {:?}", job.metadata.name);
                    new_status.project_revision = job.revision.to_owned();
                }
                Err(e) => error!("Failed to submit job with error: {:?}", e),
            }
        } else {
            info!("Processing one gordo '{}'", gordo.metadata.name);
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

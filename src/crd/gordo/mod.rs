use log::{error, info};

use crate::Controller;
use crate::crd::gordo::start_gordo_deploy_job;
use crate::crd::argo::{ArgoWorkflow, ArgoWorkflowPhase};
use crate::crd::model::{Model, filter_models_on_gordo};

pub mod gordo;
pub use gordo::*;

const WF_MATCH_LABELS: &'static [&'static str] = &[
    "applications.gordo.equinor.com/project-name", 
    "applications.gordo.equinor.com/project-revision", 
];

pub fn some_of_workflows_in_phases(workflows: &Vec<&ArgoWorkflow>, phases: Vec<ArgoWorkflowPhase>) -> bool {
    workflows.iter()
        .any(|wf| match &wf.status {
            Some(status) => (&phases).into_iter().find(|phase| &status.phase == *phase).is_some(),
            _ => false,
        })
}

pub fn all_of_workflows_in_phases(workflows: &Vec<&ArgoWorkflow>, phases: Vec<ArgoWorkflowPhase>) -> bool {
    workflows.iter()
        .all(|wf| match &wf.status {
            Some(status) => (&phases).into_iter().find(|phase| &status.phase == *phase).is_some(),
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
            let start_job_result = start_gordo_deploy_job(&gordo, &controller.client, &controller.namespace, &controller.env_config).await;
            match start_job_result {
                Ok(job) => {
                    info!("Submitted job: {:?}", job.metadata.name);
                    new_status.project_revision = job.revision.to_owned();
                }
                Err(e) => error!("Failed to submit job with error: {:?}", e),
            }
        } else {
            match orig_status.phase {
                GordoPhase::Unknown | GordoPhase::InProgress => {
                    let gordo_labels = &gordo.metadata.labels;
                    let found_workflows: Vec<&ArgoWorkflow> = workflows.iter()
                            .filter(|wf| {
                                let wf_labels = &wf.metadata.labels;
                                WF_MATCH_LABELS.
                                    iter().
                                    all(move |&label_name| wf_labels.get(label_name) == gordo_labels.get(label_name))
                            }).collect();
                    if found_workflows.len() != 0 {
                        let mut new_gordo_phase: Option<GordoPhase> = None;
                        if  some_of_workflows_in_phases(&found_workflows, vec![ArgoWorkflowPhase::Running]) {
                            new_gordo_phase = Some(GordoPhase::InProgress);
                        } else if some_of_workflows_in_phases(&found_workflows, vec![ArgoWorkflowPhase::Error, ArgoWorkflowPhase::Failed, ArgoWorkflowPhase::Skipped]) {
                            new_gordo_phase = Some(GordoPhase::BuildFailed);
                        } else if all_of_workflows_in_phases(&found_workflows, vec![ArgoWorkflowPhase::Succeeded]) {
                            new_gordo_phase = Some(GordoPhase::BuildSucceeded);
                        }
                        if let Some(phase) = new_gordo_phase {
                            info!("Apply change Gordo '{}' phase from {:?} to {:?}", gordo.metadata.name, orig_status.phase, phase);
                            new_status.phase = phase.clone();
                            match phase {
                                GordoPhase::BuildSucceeded | GordoPhase::BuildFailed => {
                                    let models = controller.model_state().await;
                                    let gordo_models: Vec<&Model> = filter_models_on_gordo(&gordo, &models).collect();
                                    if phase == GordoPhase::BuildFailed {

                                    }
                                },
                                _ => (),
                            }
                        }
                    }
                }
                _ => ()
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

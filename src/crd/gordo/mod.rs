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
                                    let mut model_patch_features: Vec<_> = Vec::with_capacity(gordo_models.len());
                                    if phase == GordoPhase::BuildFailed {
                                        let pods = controller.pod_state().await;
                                        for model in gordo_models {
                                            let model_labels = model.metadata.labels.clone();
                                            let orig_model_status = model.status.clone().unwrap_or_default();
                                            let mut new_model_status = orig_model_status.clone();
                                            new_model_status.phase = ModelPhase::BuildFailed;
                                            if let Some(model_name) = model.metadata.labels.get("applications.gordo.equinor.com/model-name") {
                                                let terminated_statuses: Vec<&ContainerStateTerminated> = pods.iter()
                                                    .filter(|pod| {
                                                        let pod_labels = &pod.metadata.labels;
                                                        POD_MATCH_LABELS.
                                                            iter().
                                                            all(|&label_name| model_labels.get(label_name) == pod_labels.get(label_name))
                                                    })
                                                    .flat_map(|pod| pod.status.as_ref())
                                                    .flat_map(|pod_status| pod_status.container_statuses.as_ref())
                                                    .flat_map(|container_statuses| container_statuses.iter().filter(|status| status.name == "main"))
                                                    .flat_map(|container_status| container_status.state.as_ref())
                                                    .flat_map(|state| state.terminated.as_ref())
                                                    .collect();
                                                if terminated_statuses.len() > 0 {
                                                    let min_date_time = MIN_DATE.clone().and_hms(0, 0, 0);
                                                    let last_terminated_state_ind = terminated_statuses.iter()
                                                        .enumerate()
                                                        .max_by_key(|(_, terminated_state)| match &terminated_state.finished_at {
                                                            Some(finished_at) => finished_at.0,
                                                            None => min_date_time,
                                                        })
                                                        .map(|(ind, _)| ind)
                                                        .unwrap_or(0);
                                                    let terminated_status = terminated_statuses[last_terminated_state_ind];
                                                    new_model_status.code = Some(terminated_status.exit_code);
                                                    if let Some(message) = &terminated_status.message {
                                                        let result: serde_json::Result<ModelPodTerminatedStatus> = serde_json::from_str(&message);
                                                        match result {
                                                            Ok(terminated_status_message) => {
                                                                new_model_status.error_type = terminated_status_message.error_type.clone();
                                                                new_model_status.message = terminated_status_message.message.clone();
                                                            },
                                                            Err(err) => warn!("Got JSON error where parsing pod's terminated message for model {}: {:?}", model_name, err),
                                                        }
                                                    }
                                                }
                                            }
                                            if new_model_status != orig_model_status {
                                                model_patch_features.push(patch_model_status(&controller.model_resource, model, new_model_status))
                                            }
                                        }
                                    } else if phase == GordoPhase::BuildSucceeded {
                                        for model in gordo_models {
                                            let orig_model_status = model.status.clone().unwrap_or_default();
                                            let mut new_model_status = orig_model_status.clone();
                                            new_model_status.phase = ModelPhase::BuildFailed;
                                            if new_model_status != orig_model_status {
                                                model_patch_features.push(patch_model_status(&controller.model_resource, model, new_model_status))
                                            }
                                        }
                                    }
                                    if model_patch_features.len() > 0 {
                                        info!("Patching statuses of {} models related to gordo with name '{}'", model_patch_features.len(), gordo.metadata.name);
                                        let results = join_all(model_patch_features).await;
                                        results.iter().for_each(|result| {
                                            if let Err(err) = result {
                                                error!("{:?}", err);
                                            }
                                        });
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

pub mod argo;
pub use argo::*;

use futures::future::join3;
use log::{error, info, warn};
use kube::api::Object;
use k8s_openapi::api::core::v1::{PodSpec, PodStatus};
use crate::crd::model::{Model, ModelPhase, ModelPodTerminatedStatus, patch_model_status};
use crate::crd::pod::{POD_MATCH_LABELS, FAILED};
use crate::Controller;
use k8s_openapi::api::core::v1::ContainerStateTerminated;
use chrono::MIN_DATE;

pub const WF_MATCH_LABELS: &'static [&'static str] = &[
    "applications.gordo.equinor.com/project-name", 
    "applications.gordo.equinor.com/project-revision", 
];

fn some_of_workflows_in_phases(workflows: &Vec<&ArgoWorkflow>, phases: Vec<ArgoWorkflowPhase>) -> bool {
    workflows.iter()
        .any(|wf| match &wf.status {
            Some(status) => match &status.phase {
                Some(status_phase) => (&phases).into_iter().find(|phase| &status_phase == phase).is_some(),
                None => false,
            },
            _ => false,
        })
}

fn all_of_workflows_in_phases(workflows: &Vec<&ArgoWorkflow>, phases: Vec<ArgoWorkflowPhase>) -> bool {
    workflows.iter()
        .all(|wf| match &wf.status {
            Some(status) => match &status.phase {
                Some(status_phase) => (&phases).into_iter().find(|phase| &status_phase == phase).is_some(),
                None => false,
            },
            _ => false,
        })
}

fn find_model_workflows<'a>(model: &'a Model, workflows: &'a [ArgoWorkflow]) -> Vec<&'a ArgoWorkflow> {
    workflows
        .iter()
        .filter(|workflow| {
            let workflow_labels = &workflow.metadata.labels;
            let model_labels = &model.metadata.labels;
            WF_MATCH_LABELS
                .iter()
                .all(move |&label_name| workflow_labels.get(label_name) == model_labels.get(label_name))
        })
        .collect()
}

fn failed_pods_terminated_statuses<'a>(model: &'a Model, pods: &'a Vec<Object<PodSpec, PodStatus>>) -> Vec<&'a ContainerStateTerminated> {
    pods.iter()
        .filter(|pod| {
            match &pod.status {
                Some(status) => match &status.phase {
                    Some(phase) => phase == FAILED,
                    None => false,
                },
                None => false,
            }
        })
        .filter(|pod| {
            let pod_labels = &pod.metadata.labels;
            let model_labels = &model.metadata.labels;
            POD_MATCH_LABELS
                .iter()
                .all(|&label_name| model_labels.get(label_name) == pod_labels.get(label_name))
        })
        .flat_map(|pod| pod.status.as_ref())
        .flat_map(|pod_status| pod_status.container_statuses.as_ref())
        .flat_map(|container_statuses| container_statuses.iter().filter(|status| status.name == "main"))
        .flat_map(|container_status| container_status.state.as_ref())
        .flat_map(|state| state.terminated.as_ref())
        .collect()
}

fn last_container_terminated_status(terminated_statuses: Vec<&ContainerStateTerminated>) -> Option<&ContainerStateTerminated> {
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
        Some(terminated_statuses[last_terminated_state_ind])
    } else {
        None
    }
}

pub async fn monitor_wf(controller: &Controller) -> () {
    let (workflows, models, pods) = join3(controller.wf_state(), controller.model_state(), controller.pod_state()).await;

    for model in models {
        match &model.status {
            Some(model_status) => match &model_status.phase {
                ModelPhase::InProgress | ModelPhase::Unknown => {
                    let found_workflows = find_model_workflows(&model, &workflows);
                    let mut new_model_phase: Option<ModelPhase> = None;
                    if some_of_workflows_in_phases(&found_workflows, vec![ArgoWorkflowPhase::Error, ArgoWorkflowPhase::Failed, ArgoWorkflowPhase::Skipped]) {
                        new_model_phase = Some(ModelPhase::Failed);
                    } else if all_of_workflows_in_phases(&found_workflows, vec![ArgoWorkflowPhase::Succeeded]) {
                        new_model_phase = Some(ModelPhase::Succeeded);
                    }
                    if let Some(model_phase) = new_model_phase {
                        let mut new_model_status = model_status.clone();
                        new_model_status.phase = model_phase.clone();
                        info!("New phase for the model '{}' will be {:?}", model.metadata.name, model_status);
                        if model_phase == ModelPhase::Failed {
                            if let Some(model_name) = model.metadata.labels.get("applications.gordo.equinor.com/model-name") {
                                let terminated_statuses = failed_pods_terminated_statuses(&model, &pods);
                                info!("Found {} failed pods in terminated status which is relates to the model '{}'", terminated_statuses.len(), model.metadata.name);
                                if let Some(terminated_status) = last_container_terminated_status(terminated_statuses) {
                                    new_model_status.code = Some(terminated_status.exit_code);
                                    if let Some(message) = &terminated_status.message {
                                        let trimmed_message = message.trim_end();
                                        if !trimmed_message.is_empty() {
                                            let result: serde_json::Result<ModelPodTerminatedStatus> = serde_json::from_str(&trimmed_message);
                                            match result {
                                                Ok(terminated_status_message) => {
                                                    info!("Last terminated status message {:?} for model '{}'", terminated_status_message, model_name);
                                                    new_model_status.error_type = terminated_status_message.error_type.clone();
                                                    new_model_status.message = terminated_status_message.message.clone();
                                                    new_model_status.traceback = terminated_status_message.traceback.clone();
                                                },
                                                Err(err) => warn!("Got JSON error where parsing pod's terminated message for the model '{}': {:?}", model_name, err),
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if model_phase != model_status.phase {
                            match patch_model_status(&controller.model_resource, &model.metadata.name, new_model_status).await {
                                Ok(new_model) => info!("Patching Model '{}' from status {:?} to {:?}", model.metadata.name, model.status, new_model.status),
                                Err(err) => error!( "Failed to patch status of Model '{}' - error: {:?}", model.metadata.name, err),                            }
                        }
                    }
                },
                _ => (),
            },
            _ => (),
        }
    }
}
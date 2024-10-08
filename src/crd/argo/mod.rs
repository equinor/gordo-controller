pub mod argo;
pub use argo::*;

use crate::crd::metrics::warning_happened;
use crate::crd::model::{
    patch_model_status, patch_model_with_default_status, Model, ModelPhase, ModelPodTerminatedStatus,
};
use crate::crd::pod::{FAILED, POD_MATCH_LABELS};
use chrono::{DateTime, Utc};
use k8s_openapi::api::core::v1::ContainerStateTerminated;
use k8s_openapi::api::core::v1::Pod;
use kube::api::Api;
use log::{error, info, warn};

pub const WF_MATCH_LABELS: &'static [&'static str] = &[
    "applications.gordo.equinor.com/project-name",
    "applications.gordo.equinor.com/project-revision",
];

pub const WF_NUMBER_LABEL: &str = "applications.gordo.equinor.com/project-workflow";

fn some_of_workflows_in_phases(workflows: &Vec<&Workflow>, phases: Vec<ArgoWorkflowPhase>) -> bool {
    workflows.iter().any(|wf| match &wf.status {
        Some(status) => match &status.phase {
            Some(status_phase) => (&phases).into_iter().find(|phase| &status_phase == phase).is_some(),
            None => false,
        },
        _ => false,
    })
}

fn all_of_workflows_in_phases(workflows: &Vec<&Workflow>, phases: Vec<ArgoWorkflowPhase>) -> bool {
    workflows.iter().all(|wf| match &wf.status {
        Some(status) => match &status.phase {
            Some(status_phase) => (&phases).into_iter().find(|phase| &status_phase == phase).is_some(),
            None => false,
        },
        _ => false,
    })
}

fn find_model_workflows<'a>(model: &'a Model, workflows: &'a [Workflow]) -> Vec<&'a Workflow> {
    //TODO for performance reason we supposed to reimplement this algorithm with BTreeMap
    workflows
        .iter()
        .filter(|workflow| {
            let workflow_labels = match &workflow.metadata.labels {
                Some(workflow_labels) => workflow_labels,
                None => {
                    return false;
                }
            };
            let model_labels = match &model.metadata.labels {
                Some(model_labels) => model_labels,
                None => {
                    return false;
                }
            };
            let equal_labels = WF_MATCH_LABELS
                .iter()
                .all(move |&label_name| workflow_labels.get(label_name) == model_labels.get(label_name));
            let mut result = equal_labels;
            if equal_labels {
                result = match (workflow_labels.get(WF_NUMBER_LABEL), model_labels.get(WF_NUMBER_LABEL)) {
                    (Some(workflow_wf_number), Some(model_wf_number)) => workflow_wf_number == model_wf_number,
                    _ => equal_labels,
                }
            }
            result
        })
        .collect()
}

fn failed_pods_terminated_statuses<'a>(model: &'a Model, pods: &'a Vec<Pod>) -> Vec<&'a ContainerStateTerminated> {
    pods.iter()
        .filter(|pod| match &pod.status {
            Some(status) => match &status.phase {
                Some(phase) => phase == FAILED,
                None => false,
            },
            None => false,
        })
        .filter(|pod| {
            let pod_labels = &pod.metadata.labels;
            let model_labels = &model.metadata.labels;
            POD_MATCH_LABELS
                .iter()
                .all(|&label_name| match (model_labels, pod_labels) {
                    (Some(model_labels), Some(pod_labels)) => {
                        model_labels.get(label_name) == pod_labels.get(label_name)
                    }
                    _ => false,
                })
        })
        .flat_map(|pod| pod.status.as_ref())
        .flat_map(|pod_status| pod_status.container_statuses.as_ref())
        .flat_map(|container_statuses| container_statuses.iter().filter(|status| status.name == "main"))
        .flat_map(|container_status| container_status.state.as_ref())
        .flat_map(|state| state.terminated.as_ref())
        .collect()
}

fn last_container_terminated_status(
    terminated_statuses: Vec<&ContainerStateTerminated>,
) -> Option<&ContainerStateTerminated> {
    if terminated_statuses.len() > 0 {
        let min_date_time = DateTime::<Utc>::MIN_UTC.clone();
        let last_terminated_state_ind = terminated_statuses
            .iter()
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

pub async fn monitor_wf(model_api: &Api<Model>, workflows: &Vec<Workflow>, models: &Vec<Model>, pods: &Vec<Pod>) -> () {
    // TODO this function definitely need to be refactored
    for model in models {
        let labels = match &model.metadata.labels {
            Some(labels) => labels,
            None => {
                warn!("Model labels field is empty");
                continue;
            }
        };
        let model_name = match &model.metadata.name {
            Some(model_name) => model_name,
            None => {
                warn!("Pod's field .metadata.name is empty");
                continue;
            }
        };
        match &model.status {
            Some(model_status) => {
                let is_reapplied_model = match (
                    &model_status.revision,
                    labels.get("applications.gordo.equinor.com/project-revision"),
                ) {
                    (Some(status_revision), Some(metadata_revision)) => status_revision != metadata_revision,
                    _ => false,
                };
                if !is_reapplied_model {
                    match &model_status.phase {
                        ModelPhase::InProgress | ModelPhase::Unknown => {
                            let found_workflows = find_model_workflows(&model, &workflows);
                            let mut new_model_phase: Option<ModelPhase> = None;
                            if some_of_workflows_in_phases(
                                &found_workflows,
                                vec![
                                    ArgoWorkflowPhase::Error,
                                    ArgoWorkflowPhase::Failed,
                                    ArgoWorkflowPhase::Skipped,
                                ],
                            ) {
                                new_model_phase = Some(ModelPhase::Failed);
                            } else if all_of_workflows_in_phases(&found_workflows, vec![ArgoWorkflowPhase::Succeeded]) {
                                new_model_phase = Some(ModelPhase::Succeeded);
                            }
                            if let Some(model_phase) = new_model_phase {
                                let mut new_model_status = model_status.clone();
                                new_model_status.phase = model_phase.clone();
                                info!("New phase for the model '{}' will be {:?}", model_name, model_status);
                                if model_phase == ModelPhase::Failed {
                                    if let Some(model_name) = labels.get("applications.gordo.equinor.com/model-name") {
                                        let terminated_statuses = failed_pods_terminated_statuses(&model, &pods);
                                        info!("Found {} failed pods in terminated status which is relates to the model '{}'", terminated_statuses.len(), model_name);
                                        if let Some(terminated_status) =
                                            last_container_terminated_status(terminated_statuses)
                                        {
                                            new_model_status.code = Some(terminated_status.exit_code);
                                            if let Some(message) = &terminated_status.message {
                                                let trimmed_message = message.trim_end();
                                                if !trimmed_message.is_empty() {
                                                    let result: serde_json::Result<ModelPodTerminatedStatus> =
                                                        serde_json::from_str(&trimmed_message);
                                                    match result {
                                                        Ok(terminated_status_message) => {
                                                            info!(
                                                                "Last terminated status message {:?} for model '{}'",
                                                                terminated_status_message, model_name
                                                            );
                                                            new_model_status.error_type =
                                                                terminated_status_message.error_type.clone();
                                                            new_model_status.message =
                                                                terminated_status_message.message.clone();
                                                            new_model_status.traceback =
                                                                terminated_status_message.traceback.clone();
                                                        }
                                                        Err(err) => {
                                                            warn!("Got JSON error where parsing pod's terminated message for the model '{}': {:?}", model_name, err);
                                                            warning_happened("parse_terminated_message")
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                if model_phase != model_status.phase {
                                    match patch_model_status(&model_api, &model_name, &new_model_status).await {
                                        Ok(new_model) => {
                                            info!(
                                                "Patching Model '{}' from status {:?} to {:?}",
                                                model_name, model.status, new_model.status
                                            );
                                        }
                                        Err(err) => {
                                            error!(
                                                "Failed to patch status of Model '{}' - error: {:?}",
                                                model_name, err
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        _ => (),
                    }
                } else {
                    match patch_model_with_default_status(&model_api, &model).await {
                        Ok(new_model) => {
                            info!(
                                "Patching Model '{}' from status {:?} to default status {:?}",
                                model_name, model.status, new_model.status
                            );
                        }
                        Err(err) => {
                            error!(
                                "Failed to patch status of Model '{}' with default status - error: {:?}",
                                model_name, err
                            );
                        }
                    }
                }
            }
            _ => (),
        };
    }
}

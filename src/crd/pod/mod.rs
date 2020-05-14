use log::{error, info};
use kube::api::Api;

use crate::Controller;
use crate::crd::model::{Model, ModelStatus, ModelPhase, patch_model_status};

pub const PENDING: &str = "Pending";
pub const RUNNING: &str = "Running";
pub const SUCCEEDED: &str = "Succeeded";
pub const FAILED: &str = "Failed";
pub const UNKNOWN: &str = "Unknown";

pub const POD_MATCH_LABELS: &'static [&'static str] = &[
    "applications.gordo.equinor.com/project-name", 
    "applications.gordo.equinor.com/project-revision", 
    "applications.gordo.equinor.com/model-name"
];

async fn update_model_status(model_resource: &Api<Model>, model: &Model, new_status: ModelStatus) {
    match patch_model_status(model_resource, &model.metadata.name, new_status).await {
        Ok(new_model) => info!("Patching Model '{}' from status {:?} to {:?}", model.metadata.name, model.status, new_model.status),
        Err(err) => error!( "Failed to patch status of Model '{}' - error: {:?}", model.metadata.name, err),
    }
}

pub async fn monitor_pods(controller: &Controller) -> () {
    let models = controller.model_state().await;

    //Filtering only active models
    let actual_models: Vec<_> = models.into_iter()
        .filter(|model| match &model.status {
            Some(status) => status.phase == ModelPhase::Unknown || status.phase == ModelPhase::InProgress,
            None => true,
        })
        .collect();
    if actual_models.is_empty() {
        return
    }

    let pods = controller.pod_state().await;

    //TODO to perform the models-pods matching in O(1) makes sense to do collect into some sort of HashMap here
    let actual_pods_labels: Vec<_> = pods.into_iter()
        .filter(|pod| pod.metadata.labels.get("applications.gordo.equinor.com/model-name").is_some())
        .flat_map(|pod| match pod.status {
            Some(status) => match status.phase {
                Some(phase) => {
                    if phase == RUNNING || phase == SUCCEEDED {
                        Some((phase, pod.metadata.labels))
                    } else {
                        None
                    }
                },
                _ => None
            }
            _ => None
        })
        .collect();

    //Update models statuses according to phases of pods which is related to each of this model
    for model in actual_models {
        let new_model_status = match &model.status {
            Some(status) => {
                let pods_labels = &actual_pods_labels;
                let pods_phases: Vec<_> = pods_labels.into_iter()
                    .filter(|(_, labels)| {
                        let model_labels = &model.metadata.labels;
                        POD_MATCH_LABELS.
                            iter().
                            all(|&label_name| model_labels.get(label_name) == labels.get(label_name))
                    })
                    .map(|(phase, _)| phase)
                    .collect();
                if pods_phases.len() > 0 {
                    info!("Found pods in phases {:?} for the model '{}'", pods_phases, model.metadata.name);
                    let mut new_status = status.clone();
                    let mut new_phase = new_status.phase.clone();
                    if pods_phases.iter().any(|phase| *phase == SUCCEEDED) {
                        new_phase = ModelPhase::Succeeded;
                    } else if pods_phases.iter().any(|phase| *phase == RUNNING) {
                        new_phase = ModelPhase::InProgress;
                    }
                    if new_phase != new_status.phase {
                        new_status.phase = new_phase;
                        Some(new_status)
                    } else {
                        None
                    }
                } else {
                    None
                }
            },
            None => Some(ModelStatus::default()),
        };
        if let Some(new_status) = new_model_status {
            update_model_status(
                &controller.model_resource,
                &model,
                new_status,
            ).await;
        }
    }
}

use log::error;
use crate::Controller;

use crate::crd::model::{Model, ModelStatus};
use kube::api::PatchParams;
use serde_json::json;

const PENDING: &str = "Pending";
const RUNNING: &str = "Running";
const SUCCEEDED: &str = "Succeeded";
const FAILED: &str = "Failed";
const UNKNOWN: &str = "Unknown";

fn pod_phase_to_model_status(phase: String) -> Option<ModelStatus> {
    let mut status = ModelStatus::Unknown;
    if phase == PENDING || phase == RUNNING {
        status = ModelStatus::InProgress;
    } else if phase == SUCCEEDED {
        status = ModelStatus::BuildSucceeded;
    } else if phase == FAILED || phase == UNKNOWN {
        status = ModelStatus::BuildFailed(1);
    }
    Some(status)
}

const MATCH_LABELS: &'static [&'static str] = &[
    "applications.gordo.equinor.com/project-name", 
    "applications.gordo.equinor.com/project-revision", 
    "applications.gordo.equinor.com/model-name"
];

pub async fn monitor_pods(controller: &Controller) -> () {
    let pods = controller.pod_state().await;
    if pods.is_empty() {
        return
    }

    let models = controller.model_state().await;

    for pod in pods {
        let pod_phase = pod.status.unwrap().phase.unwrap_or("Undefined".to_string());
        println!("Found pod '{}' in phase {}", pod.metadata.name, pod_phase);
        let pod_labels = &pod.metadata.labels;
        let found_models: Vec<&Model> = models.
            iter().
            filter(move |model| {
                let model_labels = &model.metadata.labels;
                MATCH_LABELS.
                    iter().
                    all(move |&label_name| model_labels.get(label_name) == pod_labels.get(label_name))
            }).
            collect();
        if !found_models.is_empty() {
            if found_models.len() != 1 {
                error!("Found more then one model for '{}' pod", pod.metadata.name);
                continue;
            }
            let curr_model = found_models[0];
            match (&curr_model.status, &pod_phase_to_model_status(pod_phase)) {
                (Some(curr_model_status), Some(model_status)) => {
                    if curr_model_status == model_status {
                        let patch_params = PatchParams::default();
                        let patch = serde_json::to_vec(&json!({ "status": model_status })).unwrap();
                        let name = &curr_model.metadata.name;
                        if let Err(err) = controller.model_resource.patch_status(name, &patch_params, patch).await {
                            error!( "Failed to patch status of Model '{}' - error: {:?}", name, err);
                        } else {
                            println!("Patching Model '{}' from status {:?} to {:?}", name, curr_model_status, model_status);
                        }
                    }
                },
                _ => {}
            }
        }
    }
}

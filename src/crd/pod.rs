use log::error;
use crate::Controller;

use crate::crd::model::{Model, ModelStatus};
use kube::api::PatchParams;
use serde_json::json;

use kube::api::Object;
use k8s_openapi::api::core::v1::{PodSpec, PodStatus};

struct PodPhase<'a> {
    phase: &'a str,
    model_status: ModelStatus,
}

const POD_PHASES: &'static [&'static PodPhase] = &[
    &PodPhase{phase: "Unknown", model_status: ModelStatus::BuildFailed},
    &PodPhase{phase: "Failed", model_status: ModelStatus::BuildFailed},
    &PodPhase{phase: "Pending", model_status: ModelStatus::InProgress},
    &PodPhase{phase: "Running", model_status: ModelStatus::InProgress},
    &PodPhase{phase: "Succeeded", model_status: ModelStatus::BuildSucceeded},
];

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

    for model in models {
        let model_labels = &model.metadata.labels;
        let found_pods: Vec<&Object<PodSpec, PodStatus>> = pods.
            iter().
            filter(move |pod| {
                let pod_labels = &pod.metadata.labels;
                MATCH_LABELS.
                    iter().
                    all(move |&label_name| model_labels.get(label_name) == pod_labels.get(label_name))
            }).
            collect();
        if !found_pods.is_empty() {
            let mut phases_counts: Vec<i32> = vec![0; POD_PHASES.len()]; 
            found_pods.iter()
                .flat_map(|pod| &pod.status)
                .flat_map(|status| &status.phase)
                .for_each(move |phase| {
                    for (i, pod_phase) in POD_PHASES.iter().enumerate() {
                        if pod_phase.phase == phase {
                            *&mut phases_counts[i] += 1;
                            break;
                        }
                    }
                });
            let mut last_pod_phase: Option<&PodPhase> = None;
            for (i, phases_count) in phases_counts.iter().enumerate().rev() {
                if phases_count > &0 {
                    last_pod_phase = Some(POD_PHASES[i].clone()); 
                    break
                }
            }
        }
    }

    /*for pod in pods {
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
            let model_status = pod_phase_to_model_status(pod_phase);
            if curr_model.status != model_status {
                let patch_params = PatchParams::default();
                let patch = serde_json::to_vec(&json!({ "status": model_status })).unwrap();
                let name = &curr_model.metadata.name;
                if let Err(err) = controller.model_resource.patch_status(name, &patch_params, patch).await {
                    error!( "Failed to patch status of Model '{}' - error: {:?}", name, err);
                } else {
                    println!("Patching Model '{}' from status {:?} to {:?}", name, curr_model.status, model_status);
                }
            }
        } else {
            println!("Found models list is empty")
        }
    }*/
}

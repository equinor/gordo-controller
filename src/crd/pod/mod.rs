use log::{error, info, debug};
use futures::future::join_all;
use serde_json::json;
use kube::api::{Api, Object, PatchParams};
use k8s_openapi::api::core::v1::{PodSpec, PodStatus};

use crate::Controller;
use crate::crd::model::{Model, ModelStatus, ModelPhase};

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

async fn update_model_status(model_resource: &Api<Model>, model: &Model, new_status: ModelStatus) -> () {
    let patch_params = PatchParams::default();
    let patch = serde_json::to_vec(&json!({ "status": new_status })).unwrap();
    let name = &model.metadata.name;
    if let Err(err) = model_resource.patch_status(name, &patch_params, patch).await {
        error!( "Failed to patch status of Model '{}' - error: {:?}", name, err);
    } else {
        info!("Patching Model '{}' from status {:?} to {:?}", name, model.status, new_status);
    }
}

pub async fn monitor_pods(controller: &Controller) -> () {
    let pods = controller.pod_state().await;
    //TODO to perform the pod-models matching in O(1) makes sense to do collect into some sort of HashMap here
    let running_pods: Vec<&Object<PodSpec, PodStatus>> = pods.iter()
        .filter(|pod| match pod.status.as_ref().and_then(|status| status.phase.as_ref()) {
                Some(phase) => phase == RUNNING || phase == PENDING,
                None => false
        }).collect();
    if running_pods.is_empty() {
        return
    }

    debug!("Found {} pods ins state {} or {}", running_pods.len(), RUNNING, PENDING);
    let models = controller.model_state().await;
    let status_patchers = models.iter()
        .flat_map(move |model| match &model.status {
            Some(status) => {
                if running_pods.iter()
                    .any(move |pod| {
                        let model_labels = &model.metadata.labels;
                        let pod_labels = &pod.metadata.labels;
                        POD_MATCH_LABELS.
                            iter().
                            all(move |&label_name| model_labels.get(label_name) == pod_labels.get(label_name))
                }) {
                    let mut new_status = status.clone();
                    new_status.phase = ModelPhase::InProgress;
                    Some((new_status, model))
                } else {
                    None
                }
            },
            None => Some((ModelStatus::default(), model)),
        }).map(|(new_status, model)| {
            update_model_status(
                &controller.model_resource,
                model,
                new_status,
            )
        });
    join_all(status_patchers).await;
}

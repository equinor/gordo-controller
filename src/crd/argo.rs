use crate::Controller;

use kube::api::{Api, Object};
use kube::client::APIClient;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ArgoWorkflowPhase {
    Pending,
	Running,
	Succeeded,
	Skipped,
	Failed,
	Error,
}
impl Default for ArgoWorkflowPhase {
    fn default() -> ArgoWorkflowPhase {
        ArgoWorkflowPhase::Pending
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ArgoWorkflowStatus {
    pub phase: ArgoWorkflowPhase,
}

pub type ArgoWorkflow = Object<(), ArgoWorkflowStatus>;

pub fn load_argo_workflow_resource(client: &APIClient, namespace: &str) -> Api<ArgoWorkflow> {
    Api::customResource(client.clone(), "workflows")
        .version("v1alpha1")
        .group("argoproj.io")
        .within(&namespace)
}

pub async fn monitor_wf(controller: &Controller) -> () {
    let workflows = controller.wf_state().await;

    for wf in workflows {
        let wf_phase = wf.status.unwrap().phase;
        println!("Workflow with state {:?}", wf_phase);
    }
}
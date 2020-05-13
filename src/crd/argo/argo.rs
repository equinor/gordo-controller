use kube::api::{Api, Object};
use kube::client::APIClient;
use serde::{Deserialize, Serialize};

// Origin here https://github.com/argoproj/argo/blob/master/pkg/apis/workflow/v1alpha1/workflow_types.go#L34
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ArgoWorkflowPhase {
    #[serde(alias = "pending")]
    Pending,
    #[serde(alias = "running")]
    Running,
    #[serde(alias = "succeeded")]
    Succeeded,
    #[serde(alias = "skipped")]
    Skipped,
    #[serde(alias = "failed")]
    Failed,
    #[serde(alias = "error")]
    Error,
}
impl Default for ArgoWorkflowPhase {
    fn default() -> ArgoWorkflowPhase {
        ArgoWorkflowPhase::Pending
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ArgoWorkflowSpec {
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ArgoWorkflowStatus {
    pub phase: Option<ArgoWorkflowPhase>,
}

pub type ArgoWorkflow = Object<ArgoWorkflowSpec, ArgoWorkflowStatus>;

pub fn load_argo_workflow_resource(client: &APIClient, namespace: &str) -> Api<ArgoWorkflow> {
    Api::customResource(client.clone(), "workflows")
        .version("v1alpha1")
        .group("argoproj.io")
        .within(&namespace)
}

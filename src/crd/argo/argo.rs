use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// Origin here https://github.com/argoproj/argo/blob/master/pkg/apis/workflow/v1alpha1/workflow_types.go#L34
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
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
    #[serde(alias = "omitted")]
    Omitted,
}
impl Default for ArgoWorkflowPhase {
    fn default() -> ArgoWorkflowPhase {
        ArgoWorkflowPhase::Pending
    }
}

#[derive(CustomResource, Serialize, Deserialize, Clone, Debug, Default, JsonSchema)]
#[kube(group = "argoproj.io", version = "v1alpha1", kind = "Workflow", namespaced)]
#[kube(shortname = "wf")]
#[kube(status = "ArgoWorkflowStatus")]
pub struct ArgoWorkflowSpec {}

#[derive(Serialize, Deserialize, Clone, Debug, Default, JsonSchema)]
pub struct ArgoWorkflowStatus {
    pub phase: Option<ArgoWorkflowPhase>,
}

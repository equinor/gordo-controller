use crate::crd::gordo::Gordo;
use kube::api::{Api, Object, PatchParams, Patch};
use kube::CustomResource;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::json;
use schemars::JsonSchema;

/// Represents the 'spec' field of a Model custom resource definition
#[derive(CustomResource, Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[kube(group = "equinor.com", version = "v1", status="ModelStatus", kind = "Model", namespaced)]
#[kube(shortname = "gm")]
pub struct ModelSpec {
    #[serde(rename = "gordo-version")]
    pub gordo_version: String,
    pub config: Value,
}

/// Represents the possible 'status' of a Gordo resource
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, JsonSchema)]
pub struct ModelStatus {
    pub phase: ModelPhase,
    pub code: Option<i32>,
    pub error_type: Option<String>,
    pub message: Option<String>,
    pub traceback: Option<String>,
    pub revision: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub enum ModelPhase {
    #[serde(alias = "unknown")]
    Unknown,
    #[serde(alias = "inProgress")]
    InProgress,
    #[serde(alias = "failed")]
    Failed,
    #[serde(alias = "succeeded")]
    Succeeded,
}

pub const PHASES_COUNT: usize = 4;

impl Default for ModelPhase {
    fn default() -> Self {
        ModelPhase::Unknown
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ModelPodTerminatedStatus {
    #[serde(alias = "type")]
    pub error_type: Option<String>,
    pub message: Option<String>,
    pub traceback: Option<String>,
}

/// Filter a collection of models to match a `Gordo` based on `OwnerReference`
/// and the project-revision of the `Model` matches the project-revision of the `Gordo`
pub fn filter_models_on_gordo<'a>(gordo: &'a Gordo, models: &'a [Model]) -> impl Iterator<Item = &'a Model> {
    models
        .iter()
        // Filter on OwnerReference
        .filter(move |model| {
            match (model.metadata.owner_references, gordo.metadata.name) {
                (Some(owner_references), Some(name)) => owner_references.iter().any(|owner_ref| owner_ref.name == name),
                _ => false,
            }
        })
        // Filter on matching project revision
        .filter(move |model| match gordo.status.as_ref() {
            Some(status) => {
                match model.metadata.labels {
                    Some(labels) => labels
                        .get("applications.gordo.equinor.com/project-revision")
                        // TODO: Here for compatibility when gordo-components <= 0.46.0 used 'project-version' to refer to 'project-revision'
                        // TODO: can remove when people have >= 0.47.0 of gordo
                        .or_else(|| { labels.get("applications.gordo.equinor.com/project-version") }) == Some(&status.project_revision),
                    None => false,
                }
            }
            None => false,
        })
}

pub async fn patch_model_status<'a>(model_resource: &'a Api<Model>, model_name: &'a str, new_status: &ModelStatus) -> kube::Result<Model> {
    let patch_params = PatchParams::default();
    let patch = serde_json::to_vec(&json!({ "status": new_status })).unwrap();
    model_resource.patch_status(model_name, &patch_params, &Patch::Merge(patch)).await
}

pub fn get_model_project<'a>(model: &'a Model) -> Option<String> {
  for ownerReference in &model.metadata.owner_references {
    if ownerReference.kind.eq("Gordo") {
      return Some(ownerReference.name.clone());
    }
  }
  return None;
}
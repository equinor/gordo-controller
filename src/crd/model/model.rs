use crate::crd::gordo::Gordo;
use kube::api::{Api, Object, PatchParams};
use kube::client::APIClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::json;

pub type Model = Object<ModelSpec, ModelStatus>;

/// Represents the 'spec' field of a Model custom resource definition
#[derive(Serialize, Deserialize, Clone)]
pub struct ModelSpec {
    #[serde(rename = "gordo-version")]
    pub gordo_version: String,
    pub config: Value,
}

/// Represents the possible 'status' of a Gordo resource
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ModelStatus {
    pub phase: ModelPhase,
    pub code: Option<i32>,
    pub error_type: Option<String>,
    pub message: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ModelPhase {
    #[serde(alias = "unknown")]
    Unknown,
    #[serde(alias = "inProgress")]
    InProgress,
    #[serde(alias = "buildFailed")]
    BuildFailed,
    #[serde(alias = "buildSucceeded")]
    BuildSucceeded,
}

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
}

pub fn load_model_resource(client: &APIClient, namespace: &str) -> Api<Model> {
    Api::customResource(client.clone(), "models")
        .version("v1")
        .group("equinor.com")
        .within(&namespace)
}

/// Filter a collection of models to match a `Gordo` based on `OwnerReference`
/// and the project-revision of the `Model` matches the project-revision of the `Gordo`
pub fn filter_models_on_gordo<'a>(gordo: &'a Gordo, models: &'a [Model]) -> impl Iterator<Item = &'a Model> {
    models
        .iter()
        // Filter on OwnerReference
        .filter(move |model| {
            model
                .metadata
                .ownerReferences
                .iter()
                .any(|owner_ref| owner_ref.name == gordo.metadata.name.as_str())
        })
        // Filter on matching project revision
        .filter(move |model| match gordo.status.as_ref() {
            Some(status) => {
                model
                    .metadata
                    .labels
                    .get("applications.gordo.equinor.com/project-revision")
                    // TODO: Here for compatibility when gordo-components <= 0.46.0 used 'project-version' to refer to 'project-revision'
                    // TODO: can remove when people have >= 0.47.0 of gordo
                    .or_else(|| {
                        model
                            .metadata
                            .labels
                            .get("applications.gordo.equinor.com/project-version")
                    })
                    == Some(&status.project_revision)
            }
            None => false,
        })
}

pub async fn patch_model_status<'a>(model_resource: &'a Api<Model>, model_name: &'a str, new_status: ModelStatus) -> kube::Result<Model> {
    let patch_params = PatchParams::default();
    let patch = serde_json::to_vec(&json!({ "status": new_status })).unwrap();
    model_resource.patch_status(model_name, &patch_params, patch).await
}
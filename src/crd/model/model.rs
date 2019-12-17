use crate::crd::gordo::Gordo;
use kube::api::{Api, Object};
use kube::client::APIClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Model = Object<ModelSpec, ModelStatus>;

/// Represents the 'spec' field of a Model custom resource definition
#[derive(Serialize, Deserialize, Clone)]
pub struct ModelSpec {
    #[serde(rename = "gordo-version")]
    pub gordo_version: String,
    pub config: Value,
}

/// Represents the possible 'status' of a Gordo resource
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ModelStatus {
    #[serde(alias = "unknown")]
    Unknown,
    #[serde(alias = "inProgress")]
    InProgress,
    #[serde(alias = "buildFailed")]
    BuildFailed(String),
    #[serde(alias = "buildSucceeded")]
    BuildSucceeded,
}

impl Default for ModelStatus {
    fn default() -> Self {
        ModelStatus::Unknown
    }
}

pub fn load_model_resource(client: &APIClient, namespace: &str) -> Api<Model> {
    Api::customResource(client.clone(), "models")
        .version("v1")
        .group("equinor.com")
        .within(&namespace)
}

/// Filter a collection of models to match a `Gordo` based on `OwnerReference`
/// and the project-version of the `Model` matches the project-revision of the `Gordo`
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
                    .get("applications.gordo.equinor.com/project-version")
                    == Some(&status.project_revision)
            }
            None => false,
        })
}

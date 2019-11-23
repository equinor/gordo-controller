use kube::api::Object;
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

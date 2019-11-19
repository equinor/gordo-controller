use std::collections::HashMap;

use kube::api::Object;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type GenerationNumber = Option<u32>;
pub type Gordo = Object<GordoSpec, GordoStatus>;

/// Represents the 'spec' field of a Gordo resource
#[derive(Serialize, Deserialize, Clone)]
pub struct GordoSpec {
    #[serde(rename = "deploy-version")]
    pub deploy_version: String,
    #[serde(rename = "deploy-environment")]
    pub deploy_environment: Option<HashMap<String, String>>,
    pub config: Value,
}

/// Represents the possible 'status' of a Gordo resource
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum GordoStatus {
    Submitted(GenerationNumber),
}

impl Default for GordoStatus {
    fn default() -> GordoStatus {
        GordoStatus::Submitted(None)
    }
}

use k8s_openapi::{
    apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference},
    api::core::v1::{EnvVar},
};
use kube::{
    api::{Resource},
};
use std::error::Error;
use std::fmt;

#[derive(Debug)]
struct MissingObjectKey {
    key: String,
}

impl fmt::Display for MissingObjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Key not found '{}'", self.key)
    }
}

impl Error for MissingObjectKey {}

pub fn object_to_owner_reference<K: Resource<DynamicType = ()>>(
    meta: ObjectMeta,
) -> Result<OwnerReference, Box<dyn Error>> {
    Ok(OwnerReference {
        api_version: K::api_version(&()).to_string(),
        kind: K::kind(&()).to_string(),
        name: meta.name.ok_or(MissingObjectKey{ key: ".metadata.name".to_string() })?,
        uid: meta.uid.ok_or(MissingObjectKey{ key: ".metadata.uid".to_string() })?,
        ..OwnerReference::default()
    })
}

pub fn env_var(name: &str, value: &str) -> EnvVar {
    EnvVar {
        name: name.to_string(),
        value: Some(value.to_string()),
        value_from: None,
    }
}

use k8s_openapi::{
    apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference},
    api::core::v1::{EnvVar},
};
use kube::{
    api::{Resource},
};
use crate::errors::Error;

pub fn object_to_owner_reference<K: Resource<DynamicType = ()>>(
    meta: ObjectMeta,
) -> Result<OwnerReference, Error> {
    Ok(OwnerReference {
        api_version: K::api_version(&()).to_string(),
        kind: K::kind(&()).to_string(),
        name: meta.name.ok_or(Error::MissingKey(".metadata.name"))?,
        uid: meta.uid.ok_or(Error::MissingKey(".metadata.uid"))?,
        ..OwnerReference::default()
    })
}

pub fn resource_names<T: Resource<DynamicType=()>>(resource: &Vec<T>) -> String {
    let vec: Vec<_> = resource.iter()
        .map(|resource| {
            let name = resource.meta().name.as_ref();
            format!("\"{}\"", name.unwrap_or(&"".to_string()))
        })
        .collect();
    vec.join(", ")
}

pub fn plural_str(length: usize, word: &str) -> &str {
    if length == 1 {
        word.trim_end_matches('s')
    } else {
        word
    }
}

pub fn env_var(name: &str, value: &str) -> EnvVar {
    EnvVar {
        name: name.to_string(),
        value: Some(value.to_string()),
        value_from: None,
    }
}

pub fn get_revision() -> String {
    chrono::Utc::now().timestamp_millis().to_string()
}
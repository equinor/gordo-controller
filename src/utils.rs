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
    // TODO intersperse
    let words: Vec<String> = resource.iter()
        .map(|resource| {
            match &resource.meta().name {
                Some(name) => format!("\"{}\"", name),
                None => "".into(),
            }
        })
        .collect();
    words.join(", ")
}

pub fn plural_str(length: usize, word: &str, suffix: Option<String>) -> String {
    let result = if length == 1 {
        word.trim_end_matches('s')
    } else {
        word
    };
    match (suffix, length > 0) {
        (Some(suffix_str), true) => {
            let mut suffix_owned = suffix_str.to_owned();
            suffix_owned.push_str(result);
            suffix_owned
        }
        (_, _) => result.to_string(),
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
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Key '{0}' is empty")]
    MissingKey(&'static str),

    #[error("Kube API Error: {0}")]
    KubeError(#[source] kube::Error),
}

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Key '{0}' is empty")]
    MissingKey(&'static str),

    #[error("Kube API Error: {0}")]
    KubeError(#[source] kube::Error),

    #[error("Tokio JoinError: {0}")]
    TokioJoinError(#[source] tokio::task::JoinError),

    #[error("{0} is empty")]
    NotFound(&'static str),
}

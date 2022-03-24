use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Key '{0}' is empty")]
    MissingKey(&'static str),

    #[error("Kube API Error: {0}")]
    KubeError(#[source] kube::Error),

    #[error("{0} is empty")]
    NotFound(&'static str),
}

impl Error {
    pub fn error_name(self) -> &'static str {
        match self {
            Error::MissingKey(_) => "missing_key",
            Error::KubeError(_) => "kube_error",
            Error::NotFound(_) => "not_found",
        }
    }
}

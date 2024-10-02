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

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to load environment config: {0}")]
    Environment(#[source] envy::Error),
    #[error("Faild to load '{0}' config field: {1}")]
    Field(&'static str, String),
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

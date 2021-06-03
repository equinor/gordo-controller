use prometheus::{Opts, IntCounterVec, Registry};

use lazy_static::lazy_static;
use kube::{Error};

pub const METRICS_NAMESPACE: &str = "gordo_controller";

lazy_static! {
    pub static ref KUBE_ERRORS: IntCounterVec = IntCounterVec::new(
      Opts::new("kube_errors", "gordo-controller k8s related errors")
      .namespace(METRICS_NAMESPACE),
      &["action", "kube_name"]
    ).unwrap();
    pub static ref ERRORS: IntCounterVec = IntCounterVec::new(
      Opts::new("errors", "gordo-controller errors")
      .namespace(METRICS_NAMESPACE),
      &["name"]
    ).unwrap();
}

pub fn custom_metrics(registry: &Registry) {
  registry.register(Box::new(KUBE_ERRORS.clone())).unwrap();
  registry.register(Box::new(ERRORS.clone())).unwrap();
}

pub fn kube_error_name<'a>(err: Error) -> &'a str {
  match err {
    Error::Api(_) => "api",
    Error::ReqwestError(_) => "request_error",
    Error::HttpError(_) => "http_error",
    Error::SerdeError(_) => "serde_error",
    Error::RequestBuild => "request_build",
    Error::RequestSend => "request_send",
    Error::RequestParse => "request_parse",
    Error::InvalidMethod(_) => "request_method",
    Error::RequestValidation(_) => "request_validation",
    Error::KubeConfig(_) => "kube_config",
    Error::SslError(_) => "ssl_error",
  }
}

pub fn kube_error_happened(action: &str, err: Error) {
  KUBE_ERRORS.with_label_values(&[action, kube_error_name(err)]).inc_by(1);
}

pub fn error_happened(name: &str) {
  ERRORS.with_label_values(&[name]).inc_by(1);
}

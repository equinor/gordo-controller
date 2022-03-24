use prometheus::{Opts, IntCounterVec, Registry};
use lazy_static::lazy_static;

pub const METRICS_NAMESPACE: &str = "gordo_controller";

lazy_static! {
    pub static ref KUBE_ERRORS: IntCounterVec = IntCounterVec::new(
      Opts::new("kube_errors", "gordo-controller k8s related errors")
      .namespace(METRICS_NAMESPACE),
      &["action", "kube_name"]
    ).unwrap();
    pub static ref WARNINGS: IntCounterVec = IntCounterVec::new(
      Opts::new("warnings", "gordo-controller warnings")
      .namespace(METRICS_NAMESPACE),
      &["name"]
    ).unwrap();
    pub static ref RECONCILE_GORDO_COUNT: IntCounterVec = IntCounterVec::new(
      Opts::new("reconcile_count", "Gordo reconcile count")
      .namespace(METRICS_NAMESPACE),
      &["gordo_name"]
    ).unwrap();
    pub static ref RECONCILE_GORDO_ERROR: IntCounterVec = IntCounterVec::new(
      Opts::new("reconcile_gordo_error", "Reconcile Gordo errors")
      .namespace(METRICS_NAMESPACE),
      &[]
    ).unwrap();
}

pub fn custom_metrics(registry: &Registry) {
  registry.register(Box::new(KUBE_ERRORS.clone())).unwrap();
  registry.register(Box::new(WARNINGS.clone())).unwrap();
  registry.register(Box::new(RECONCILE_GORDO_COUNT.clone())).unwrap();
  registry.register(Box::new(RECONCILE_GORDO_ERROR.clone())).unwrap();
}

pub fn warning_happened(name: &str) {
  WARNINGS.with_label_values(&[name]).inc_by(1);
}
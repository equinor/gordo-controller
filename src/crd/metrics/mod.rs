use crate::crd::model::{ModelPhase, PHASES_COUNT};

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use prometheus::{Opts, IntCounterVec, IntGaugeVec, Registry};
use lazy_static::lazy_static;
use kube::{Error};

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
    pub static ref MODEL_COUNTS: IntGaugeVec = IntGaugeVec::new(
      Opts::new("model_counts", "Number of models per projects and phases")
      .namespace(METRICS_NAMESPACE),
      &["project", "phase"]
    ).unwrap();
    pub static ref GORDO_PROJECTS: IntGaugeVec = IntGaugeVec::new(
      Opts::new("gordo_projects", "One metric per gordo project")
      .namespace(METRICS_NAMESPACE),
      &["project"]
    ).unwrap();
    pub static ref GORDO_PULLING: IntCounterVec = IntCounterVec::new(
      Opts::new("resource_pulling_counts", "Pulling resource count")
      .namespace(METRICS_NAMESPACE)
      .const_label("name", "gordo"),
      &[]
    ).unwrap();
    pub static ref MODEL_PULLING: IntCounterVec = IntCounterVec::new(
      Opts::new("resource_pulling_counts", "Pulling resource count")
      .namespace(METRICS_NAMESPACE)
      .const_label("name", "model"),
      &[]
    ).unwrap();
    pub static ref POD_PULLING: IntCounterVec = IntCounterVec::new(
      Opts::new("resource_pulling_counts", "Pulling resource count")
      .namespace(METRICS_NAMESPACE)
      .const_label("name", "pod"),
      &[]
    ).unwrap();
    pub static ref ARGO_PULLING: IntCounterVec = IntCounterVec::new(
      Opts::new("resource_pulling_counts", "Pulling resource count")
      .namespace(METRICS_NAMESPACE)
      .const_label("name", "argo"),
      &[]
    ).unwrap();
    pub static ref RECONCILE_ERROR: IntCounterVec = IntCounterVec::new(
      Opts::new("reconcile_error", "Controller reconcile errors")
      .namespace(METRICS_NAMESPACE)
      &[]
    ).unwrap();
    pub static ref PROJECTS: Mutex<HashMap<String, bool>> = Mutex::new(HashMap::new());
}

pub fn custom_metrics(registry: &Registry) {
  registry.register(Box::new(KUBE_ERRORS.clone())).unwrap();
  registry.register(Box::new(WARNINGS.clone())).unwrap();
  registry.register(Box::new(MODEL_COUNTS.clone())).unwrap();
  registry.register(Box::new(GORDO_PROJECTS.clone())).unwrap();
  registry.register(Box::new(GORDO_PULLING.clone())).unwrap();
  registry.register(Box::new(MODEL_PULLING.clone())).unwrap();
  registry.register(Box::new(POD_PULLING.clone())).unwrap();
  registry.register(Box::new(ARGO_PULLING.clone())).unwrap();
  registry.register(Box::new(RECONCILE_ERROR.clone())).unwrap();
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

pub fn warning_happened(name: &str) {
  WARNINGS.with_label_values(&[name]).inc_by(1);
}


//Number of phases
const INITIAL_PROJECT_COUNT: usize = 5;

pub struct ModelPhasesMetrics {
  projects: HashMap<String, usize>,
  metrics: Vec<i64>,
  next_index: usize,
}

impl ModelPhasesMetrics {

  pub fn new(initial_projects_count: Option<u32>) -> Self {
    let project_count = initial_projects_count.unwrap_or(INITIAL_PROJECT_COUNT as u32) as usize;
    ModelPhasesMetrics {
      projects: HashMap::with_capacity(project_count),
      metrics: Vec::with_capacity(project_count * PHASES_COUNT),
      next_index: 0,
    }
  }

  fn get_index(phase: ModelPhase) -> usize {
    match phase {
      ModelPhase::Unknown => 0,
      ModelPhase::InProgress => 1,
      ModelPhase::Succeeded => 2,
      ModelPhase::Failed => 3,
    }
  }

  fn get_project_index(&mut self, project: String) -> usize {
    match self.projects.get(&project) {
      Some(index) => *index,
      None => {
        let index = self.next_index;
        let next_index = index + PHASES_COUNT;
        self.metrics.resize(next_index, 0);
        self.projects.insert(project, index);
        self.next_index = next_index;
        index
      }
    }
  }

  pub fn inc_model_counts(&mut self, project: String, phase: ModelPhase) {
    let base_index = self.get_project_index(project);
    let index = base_index + Self::get_index(phase);
    self.metrics[index] = self.metrics[index] + 1;
  }
}

fn phase_labels<'a>() -> [(ModelPhase, &'a str); PHASES_COUNT] {
  return [
    (ModelPhase::Unknown, "unknown"),
    (ModelPhase::InProgress, "in_progress"),
    (ModelPhase::Succeeded, "succeeded"),
    (ModelPhase::Failed, "failed"),
  ];
}

pub fn update_gordo_projects(gordo_projects: &HashSet<String>) {
  // TODO consider to return Result<...> from this function
  let mut old_project = PROJECTS.lock().unwrap();
  for (project, exists) in old_project.iter_mut() {
    let new_exists = gordo_projects.contains(project);
    if !new_exists {
      GORDO_PROJECTS.with_label_values(&[project]).set(0);
      *exists = false;
    }
  }
  for project in gordo_projects {
    GORDO_PROJECTS.with_label_values(&[project]).set(1);
    old_project.insert(project.clone(), true);
  }
}

pub fn update_model_counts(model_phases_metrics: &ModelPhasesMetrics) {
  // TODO consider to return Result<...> from this function
  let old_project = PROJECTS.lock().unwrap();
  let new_projects = &model_phases_metrics.projects;
  let mut labels: [&str; 2] = ["", ""];
  let phase_labels = phase_labels();
  for (project, exists) in old_project.iter() {
    labels[0] = project;
    for (model_phase, phase_label) in &phase_labels {
      labels[1] = phase_label;
      let mut metric: i64 = 0;
      if *exists {
        // TODO move this part to ModelPhasesMetrics
        metric = match new_projects.get(project) {
          Some(base_index) => {
            let index = base_index + ModelPhasesMetrics::get_index(model_phase.clone());
            model_phases_metrics.metrics[index]
          }
          None => 0
        }
      }
      MODEL_COUNTS.with_label_values(&labels).set(metric);
    }
  }
}

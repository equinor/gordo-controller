pub mod argo;
pub use argo::*;

use crate::Controller;

pub async fn monitor_wf(controller: &Controller) -> () {
    let workflows = controller.wf_state().await;

    for wf in workflows {
        let wf_phase = wf.status.unwrap().phase;
        println!("Workflow with state {:?}", wf_phase);
    }
}
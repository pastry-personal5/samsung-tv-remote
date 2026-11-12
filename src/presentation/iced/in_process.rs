//! Temporary M7 transport adapter. This owns a lifetime and a client handle,
//! never TV state, session handles, or persistence policy.
use crate::application::control_service::{ClientId, ControlService};
use crate::application::runtime::{Runtime, RuntimeLimits};
use crate::application::tv_control_coordinator::AppServices;
use std::sync::Mutex;

pub struct InProcess {
    runtime: Mutex<Option<Runtime>>,
    pub service: ControlService,
}
impl std::fmt::Debug for InProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InProcess")
    }
}
impl InProcess {
    pub fn start(services: AppServices) -> Self {
        let runtime = Runtime::start(services, RuntimeLimits::default());
        let service = runtime.client(ClientId(1));
        Self {
            runtime: Mutex::new(Some(runtime)),
            service,
        }
    }
    pub async fn drain(&self) {
        let runtime = self.runtime.lock().ok().and_then(|mut slot| slot.take());
        if let Some(runtime) = runtime {
            runtime.drain().await;
        }
    }
}

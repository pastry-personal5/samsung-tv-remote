pub mod application;
pub mod control_client;
pub mod domain;
pub mod infrastructure;
pub mod presentation;
pub mod server;

pub use application::ControlState;
pub use application::SendRemoteAction;
pub use domain::{DeviceDisplay, DeviceId, RemoteAction};

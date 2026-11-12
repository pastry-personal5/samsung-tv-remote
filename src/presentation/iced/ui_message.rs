use super::view_model::PrimaryView;
use crate::application::control_service::{Receipt, ServiceError, Snapshot};
use crate::application::tv_address::TvHost;
use crate::application::wake::WakeInterface;
use crate::application::SendRemoteAction;
use crate::domain::DeviceId;
use crate::domain::RemoteAction;
use ::iced::window;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsPage {
    #[default]
    Discovery,
    WakeOnLan,
}

#[derive(Debug, Clone)]
pub enum Message {
    OpenMainWindow,
    Navigate(PrimaryView),
    OpenSettings,
    SelectSettingsPage(SettingsPage),
    MainWindowOpened(window::Id),
    SettingsWindowOpened(window::Id),
    WindowClosed(window::Id),
    ResizeMessages(u16),
    FeedScrolled {
        at_bottom: bool,
    },
    Shortcut {
        window: window::Id,
        shortcut: Shortcut,
    },
    /// Typed remote action intent; disabled controls never emit it.
    AttemptRemoteAction(SendRemoteAction),
    PowerToggle(SendRemoteAction),
    Wake,
    CancelWake,
    WakeTick,
    WakeWiredChanged(String),
    WakeWifiChanged(String),
    WakeInterfaceSelected(WakeInterface),
    WakeInterfaceCleared,
    TvAddressChanged(String),
    DiscoverTv,
    UseCandidate(TvHost),
    ProbeCandidate(TvHost),
    ProbeTv,
    /// Checks the selected saved TV without opening Settings.
    CheckSelectedTv,
    ConfirmAndPair,
    ConfirmAndRepair,
    ConnectSelected,
    ForgetSelected,
    SelectSaved(DeviceId),
    RuntimeStarted(Arc<super::in_process::InProcess>),
    ServiceSnapshot(Snapshot),
    ServiceResnapshot(Snapshot),
    ServiceResult(Receipt),
    Admitted(u64, Result<Receipt, ServiceError>),
    ServiceUnavailable,
    Drained,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Navigate(PrimaryView),
    OpenSettings,
    Remote(RemoteAction),
}

use super::view_model::PrimaryView;
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
    Sources,
    Servers,
}

/// One explicit edit of a source key; Show needs a second confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceEdit {
    Show,
    Hide,
    Reset,
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
    ReloadWakeSettings,
    /// One finite source-key request for the selected TV.
    SendSource(RemoteAction),
    EditSource(RemoteAction, SourceEdit),
    ConfirmShowSource,
    CancelShowSource,
    SourceLabelChanged(RemoteAction, String),
    SaveSourceLabel(RemoteAction),
    OpenSourceSettings,
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
    RemoteStarted(
        Result<
            Arc<dyn crate::control_client::port::NativeClient>,
            crate::control_client::port::ManagementError,
        >,
    ),
    RemoteUpdate(crate::control_client::background::Update),
    RemoteAdmitted(
        u64,
        Result<
            crate::control_client::projection::NativeReceipt,
            crate::control_client::background::SubmitError,
        >,
    ),
    ResumeServer,
    RestartServer,
    RecheckServer,
    RequestStopServer,
    ConfirmStopServer,
    CancelStopServer,
    CheckMcp,
    VerifyMcp,
    ConnectMcp,
    RequestRepairMcp,
    ConfirmRepairMcp,
    CancelRepairMcp,
    RequestDisconnectMcp,
    ConfirmDisconnectMcp,
    CancelDisconnectMcp,
    RetryMcp,
    McpFinished(Result<crate::control_client::mcp::State, crate::control_client::mcp::Error>),
    RemoteManagementFinished(
        Result<
            crate::control_client::port::ManagementReply,
            crate::control_client::port::ManagementError,
        >,
    ),
    CaptureProbe,
    ProbeCaptured(window::Screenshot),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Navigate(PrimaryView),
    OpenSettings,
    Remote(RemoteAction),
}

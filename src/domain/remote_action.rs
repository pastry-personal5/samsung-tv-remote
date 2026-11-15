/// Finite semantic actions exposed by the Remote View.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RemoteAction {
    /// Toggle the TV power state.
    PowerToggle,
    Up,
    Down,
    Left,
    Right,
    Enter,
    Back,
    Home,
    /// A visual-remote intent resolved to Play or Pause by the application.
    PlayPause,
    /// Start playback on the TV.
    Play,
    /// Pause playback on the TV.
    Pause,
    Mute,
    VolumeUp,
    VolumeDown,
    /// Direct-input keys. A write is only a request; no reply proves the input.
    Hdmi1,
    Hdmi2,
    Hdmi3,
    Tuner,
    /// Opens the TV's own Sources menu for manual D-pad use.
    SourceMenu,
}

impl RemoteAction {
    pub const LIVE_ACTIONS: [Self; 11] = [
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::Enter,
        Self::Back,
        Self::Home,
        Self::PlayPause,
        Self::Mute,
        Self::VolumeUp,
        Self::VolumeDown,
    ];

    /// Actions that can be encoded as Samsung remote keys.
    pub const ENCODED_ACTIONS: [Self; 17] = [
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::Enter,
        Self::Back,
        Self::Home,
        Self::Play,
        Self::Pause,
        Self::Mute,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::Hdmi1,
        Self::Hdmi2,
        Self::Hdmi3,
        Self::Tuner,
        Self::SourceMenu,
    ];

    /// Source keys the user configures per TV, in storage and wire order.
    pub const SOURCE_PORTS: [Self; 4] = [Self::Hdmi1, Self::Hdmi2, Self::Hdmi3, Self::Tuner];

    /// The one action-to-capability map. `SourceMenu` maps to `sources`.
    pub const fn source_capability(self) -> Option<super::capability::CapabilityKey> {
        use super::capability::CapabilityKey;
        match self {
            Self::Hdmi1 => Some(CapabilityKey::Hdmi1),
            Self::Hdmi2 => Some(CapabilityKey::Hdmi2),
            Self::Hdmi3 => Some(CapabilityKey::Hdmi3),
            Self::Tuner => Some(CapabilityKey::Tuner),
            Self::SourceMenu => Some(CapabilityKey::Sources),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::capability::CapabilityKey;

    #[test]
    fn source_keys_are_encoded_but_not_live_and_map_to_their_facts() {
        for action in [
            RemoteAction::SOURCE_PORTS.as_slice(),
            &[RemoteAction::SourceMenu],
        ]
        .concat()
        {
            assert!(RemoteAction::ENCODED_ACTIONS.contains(&action));
            assert!(!RemoteAction::LIVE_ACTIONS.contains(&action));
        }
        assert_eq!(
            RemoteAction::SOURCE_PORTS.map(|a| a.source_capability().unwrap()),
            [
                CapabilityKey::Hdmi1,
                CapabilityKey::Hdmi2,
                CapabilityKey::Hdmi3,
                CapabilityKey::Tuner
            ]
        );
        assert_eq!(
            RemoteAction::SourceMenu.source_capability(),
            Some(CapabilityKey::Sources)
        );
        assert_eq!(RemoteAction::Home.source_capability(), None);
        assert_eq!(
            serde_json::to_value(RemoteAction::SourceMenu).unwrap(),
            "SourceMenu"
        );
        assert_eq!(serde_json::to_value(RemoteAction::Hdmi1).unwrap(), "Hdmi1");
    }
}

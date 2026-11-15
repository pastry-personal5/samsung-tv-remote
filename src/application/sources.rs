//! Per-TV direct-input preferences. No probe can prove a source key, so the
//! user's own choice is the only evidence, and it governs admission.
use crate::domain::RemoteAction;

pub const MAX_LABEL_SCALARS: usize = 24;
pub const MAX_LABEL_BYTES: usize = 64;

/// A validated, trimmed user label. Invalid content is rejected, never stripped.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceLabel(String);

impl SourceLabel {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let forbidden = |c: char| {
            c.is_control()
                || matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        };
        (!value.is_empty()
            && value.chars().count() <= MAX_LABEL_SCALARS
            && value.len() <= MAX_LABEL_BYTES
            && !value.chars().any(forbidden))
        .then(|| Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a client asks for. The owner assigns confirmation times.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum SourceChoice {
    #[default]
    Unconfigured,
    Hidden,
    Shown(Option<SourceLabel>),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SourceSetting {
    #[default]
    Unconfigured,
    /// A preference, not negative evidence.
    Hidden,
    Shown {
        label: Option<SourceLabel>,
        /// When the user first showed this key. Relabelling keeps it.
        confirmed_at_unix_ms: u64,
    },
}

impl SourceSetting {
    pub fn choice(&self) -> SourceChoice {
        match self {
            Self::Unconfigured => SourceChoice::Unconfigured,
            Self::Hidden => SourceChoice::Hidden,
            Self::Shown { label, .. } => SourceChoice::Shown(label.clone()),
        }
    }
}

/// One entry per `RemoteAction::SOURCE_PORTS` key, in that order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceConfiguration {
    /// Boxed so retained receipts carrying a `SavedDevice` stay small.
    pub ports: Box<[SourceSetting; 4]>,
}

impl SourceConfiguration {
    pub fn setting(&self, action: RemoteAction) -> Option<&SourceSetting> {
        let index = RemoteAction::SOURCE_PORTS
            .iter()
            .position(|a| *a == action)?;
        Some(&self.ports[index])
    }
    pub fn choices(&self) -> [SourceChoice; 4] {
        (*self.ports).clone().map(|setting| setting.choice())
    }
    /// Full replacement. Showing establishes confirmation; hiding or reset clears it.
    pub fn apply(&self, choices: [SourceChoice; 4], now_unix_ms: u64) -> Self {
        let mut index = 0;
        Self {
            ports: Box::new(choices.map(|choice| {
                let previous = &self.ports[index];
                index += 1;
                match choice {
                    SourceChoice::Unconfigured => SourceSetting::Unconfigured,
                    SourceChoice::Hidden => SourceSetting::Hidden,
                    SourceChoice::Shown(label) => SourceSetting::Shown {
                        label,
                        confirmed_at_unix_ms: match previous {
                            SourceSetting::Shown {
                                confirmed_at_unix_ms,
                                ..
                            } => *confirmed_at_unix_ms,
                            _ => now_unix_ms.max(1),
                        },
                    },
                }
            })),
        }
    }
}

/// Canonical port identity, shown even beside a custom label.
pub fn port_name(action: RemoteAction) -> &'static str {
    match action {
        RemoteAction::Hdmi1 => "HDMI 1",
        RemoteAction::Hdmi2 => "HDMI 2",
        RemoteAction::Hdmi3 => "HDMI 3",
        RemoteAction::Tuner => "TV tuner",
        RemoteAction::SourceMenu => "Sources menu",
        _ => "",
    }
}

pub(crate) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_trimmed_bounded_and_unsafe_content_is_rejected() {
        assert_eq!(
            SourceLabel::parse("  PlayStation \n").unwrap().as_str(),
            "PlayStation"
        );
        assert!(SourceLabel::parse("   ").is_none());
        assert!(SourceLabel::parse(&"a".repeat(24)).is_some());
        assert!(SourceLabel::parse(&"a".repeat(25)).is_none());
        // 16 four-byte scalars fit the scalar limit but exceed 64 bytes.
        assert!(SourceLabel::parse(&"😀".repeat(16)).is_some());
        assert!(SourceLabel::parse(&"😀".repeat(17)).is_none());
        for bad in [
            "a\u{0007}b",
            "a\u{0085}b",
            "a\u{202E}b",
            "a\u{2066}b",
            "a\u{200F}b",
            "a\u{061C}b",
        ] {
            assert!(SourceLabel::parse(bad).is_none(), "{bad:?}");
        }
        // Combining marks and emoji joiners are ordinary content.
        assert!(SourceLabel::parse("e\u{0301}").is_some());
        assert!(SourceLabel::parse("👩\u{200D}💻").is_some());
    }

    #[test]
    fn confirmation_time_survives_relabel_and_clears_on_hide_or_reset() {
        let shown = |l: &str| SourceChoice::Shown(SourceLabel::parse(l));
        let first = SourceConfiguration::default().apply(
            [
                shown("PS5"),
                SourceChoice::Hidden,
                SourceChoice::Unconfigured,
                shown("x"),
            ],
            100,
        );
        assert!(matches!(
            first.ports[0],
            SourceSetting::Shown {
                confirmed_at_unix_ms: 100,
                ..
            }
        ));
        let relabel = first.apply(
            [
                shown("Console"),
                SourceChoice::Shown(None),
                SourceChoice::Unconfigured,
                SourceChoice::Hidden,
            ],
            200,
        );
        assert_eq!(
            relabel.ports[0],
            SourceSetting::Shown {
                label: SourceLabel::parse("Console"),
                confirmed_at_unix_ms: 100
            }
        );
        assert!(matches!(
            relabel.ports[1],
            SourceSetting::Shown {
                confirmed_at_unix_ms: 200,
                ..
            }
        ));
        assert_eq!(relabel.ports[3], SourceSetting::Hidden);
        let again = relabel.apply(
            [
                shown("PS5"),
                SourceChoice::Unconfigured,
                SourceChoice::Unconfigured,
                shown("x"),
            ],
            300,
        );
        assert!(matches!(
            again.ports[3],
            SourceSetting::Shown {
                confirmed_at_unix_ms: 300,
                ..
            }
        ));
        assert_eq!(
            again.setting(RemoteAction::Hdmi2),
            Some(&SourceSetting::Unconfigured)
        );
        assert_eq!(again.setting(RemoteAction::SourceMenu), None);
    }
}

//! Bounded, in-memory evidence policy. The owner supplies time and session identity.
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::Instant;

use super::control_service::Epoch;
use crate::domain::capability::*;
use crate::domain::DeviceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceContext {
    pub epoch: Epoch,
    pub selection_generation: u64,
    pub session: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityFact {
    pub key: CapabilityKey,
    pub support: Support,
    pub source: EvidenceSource,
    pub observed_at_unix_ms: Option<u64>,
    pub context: Option<EvidenceContext>,
    pub freshness: Freshness,
}

impl CapabilityFact {
    pub fn unknown(key: CapabilityKey) -> Self {
        Self {
            key,
            support: Support::Unknown,
            source: EvidenceSource::None,
            observed_at_unix_ms: None,
            context: None,
            freshness: Freshness::Unobserved,
        }
    }

    /// Reject contradictory evidence before a compatible client can display it.
    pub fn valid(&self) -> bool {
        if self.support == Support::Unknown {
            return self == &Self::unknown(self.key);
        }
        self.observed_at_unix_ms.is_some_and(|t| t > 0)
            && self.context.is_some_and(|c| c.session > 0)
            && self.freshness != Freshness::Unobserved
            && match self.key {
                CapabilityKey::InstalledApps | CapabilityKey::AppLaunch => matches!(
                    self.source,
                    EvidenceSource::TvResponse | EvidenceSource::UserConfirmation
                ),
                CapabilityKey::NativeIme => {
                    self.source == EvidenceSource::TvEvent && self.support == Support::Supported
                }
                _ => self.source == EvidenceSource::UserConfirmation,
            }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySummary {
    pub device: DeviceId,
    pub reason: CapabilityReason,
    pub ime_active: bool,
    pub facts: [CapabilityFact; 8],
}
impl CapabilitySummary {
    pub fn unknown(device: DeviceId, reason: CapabilityReason) -> Self {
        Self {
            device,
            reason,
            ime_active: false,
            facts: CapabilityKey::ALL.map(CapabilityFact::unknown),
        }
    }
    pub fn fact(&self, key: CapabilityKey) -> &CapabilityFact {
        self.facts
            .iter()
            .find(|f| f.key == key)
            .expect("finite capability set")
    }
    pub fn reason_for(&self, key: CapabilityKey) -> CapabilityReason {
        if self.reason != CapabilityReason::Connected {
            return self.reason;
        }
        match self.fact(key) {
            CapabilityFact {
                freshness: Freshness::Unobserved,
                ..
            } => CapabilityReason::EvidenceMissing,
            CapabilityFact {
                freshness: Freshness::Stale,
                ..
            } => CapabilityReason::EvidenceStale,
            CapabilityFact {
                support: Support::Unsupported,
                ..
            } => CapabilityReason::FeatureUnsupported,
            _ => CapabilityReason::Observed,
        }
    }
}

struct Record {
    fact: CapabilityFact,
    expires: Option<Instant>,
}

pub(super) struct CapabilityObservation {
    pub key: CapabilityKey,
    pub support: Support,
    pub source: EvidenceSource,
    pub wall_ms: u64,
    pub ttl: Option<Duration>,
}

#[derive(Default)]
pub(super) struct CapabilityStore {
    history: HashMap<(DeviceId, CapabilityKey), Record>,
    active: Option<(DeviceId, EvidenceContext)>,
    ime_active: bool,
}

impl CapabilityStore {
    pub fn session(&mut self, active: Option<(DeviceId, EvidenceContext)>) {
        if self.active != active {
            for record in self.history.values_mut() {
                record.fact.freshness = Freshness::Stale;
            }
            self.ime_active = false;
            self.active = active;
        }
    }
    pub fn retain(&mut self, devices: &[super::device_repository::SavedDevice]) {
        if self
            .active
            .is_some_and(|(id, _)| !devices.iter().any(|d| d.id == id))
        {
            self.session(None);
        }
        self.history
            .retain(|(id, _), _| devices.iter().any(|d| d.id == *id));
    }
    pub fn observe(
        &mut self,
        device: DeviceId,
        context: EvidenceContext,
        observation: CapabilityObservation,
        now: Instant,
    ) -> bool {
        let CapabilityObservation {
            key,
            support,
            source,
            wall_ms,
            ttl,
        } = observation;
        if self.active != Some((device, context)) {
            return false;
        }
        let fact = CapabilityFact {
            key,
            support,
            source,
            observed_at_unix_ms: Some(wall_ms),
            context: Some(context),
            freshness: Freshness::Current,
        };
        if !fact.valid() {
            return false;
        }
        let expires = match ttl {
            Some(ttl) => match now.checked_add(ttl) {
                Some(t) => Some(t),
                None => return false,
            },
            None => None,
        };
        // Repeated events are idempotent; they do not silently extend a deadline.
        if self.history.get(&(device, key)).is_some_and(|r| {
            r.fact.context == Some(context)
                && r.fact.support == support
                && r.fact.source == source
                && r.fact.freshness == Freshness::Current
        }) {
            return false;
        }
        self.history.insert((device, key), Record { fact, expires });
        true
    }
    pub fn ime(
        &mut self,
        device: DeviceId,
        context: EvidenceContext,
        active: bool,
        wall_ms: u64,
        now: Instant,
    ) -> bool {
        if wall_ms == 0 || self.active != Some((device, context)) {
            return false;
        }
        let observed = self.observe(
            device,
            context,
            CapabilityObservation {
                key: CapabilityKey::NativeIme,
                support: Support::Supported,
                source: EvidenceSource::TvEvent,
                wall_ms,
                ttl: None,
            },
            now,
        );
        let changed = self.ime_active != active;
        // An unmatched end can establish lifecycle support, never an active field.
        self.ime_active = active;
        observed || changed
    }
    pub fn expire(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for record in self.history.values_mut() {
            if record.fact.freshness == Freshness::Current
                && record.expires.is_some_and(|t| t <= now)
            {
                record.fact.freshness = Freshness::Stale;
                if record.fact.key == CapabilityKey::NativeIme {
                    self.ime_active = false;
                }
                changed = true;
            }
        }
        changed
    }
    pub fn summary(&self, device: DeviceId, reason: CapabilityReason) -> CapabilitySummary {
        CapabilitySummary {
            device,
            reason,
            ime_active: reason == CapabilityReason::Connected
                && self.active.is_some_and(|(id, _)| id == device)
                && self.ime_active,
            facts: CapabilityKey::ALL.map(|key| {
                self.history
                    .get(&(device, key))
                    .map(|r| r.fact.clone())
                    .unwrap_or_else(|| CapabilityFact::unknown(key))
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context(generation: u64, session: u64) -> EvidenceContext {
        EvidenceContext {
            epoch: Epoch::from_u128(1),
            selection_generation: generation,
            session,
        }
    }
    fn observe(
        store: &mut CapabilityStore,
        key: CapabilityKey,
        source: EvidenceSource,
        wall_ms: u64,
        now: Instant,
    ) -> bool {
        store.observe(
            DeviceId::new(1),
            context(1, 1),
            CapabilityObservation {
                key,
                support: Support::Supported,
                source,
                wall_ms,
                ttl: Some(Duration::from_secs(5)),
            },
            now,
        )
    }
    #[test]
    fn evidence_is_per_feature_and_never_inferred_from_an_unrelated_response() {
        let mut store = CapabilityStore::default();
        store.session(Some((DeviceId::new(1), context(1, 1))));
        assert!(observe(
            &mut store,
            CapabilityKey::InstalledApps,
            EvidenceSource::TvResponse,
            10,
            Instant::now()
        ));
        assert!(!observe(
            &mut store,
            CapabilityKey::Hdmi1,
            EvidenceSource::TvResponse,
            10,
            Instant::now()
        ));
        let summary = store.summary(DeviceId::new(1), CapabilityReason::Connected);
        assert_eq!(
            summary.reason_for(CapabilityKey::AppLaunch),
            CapabilityReason::EvidenceMissing
        );
        assert_eq!(summary.fact(CapabilityKey::Hdmi1).support, Support::Unknown);
        assert_eq!(
            store
                .summary(DeviceId::new(2), CapabilityReason::NotSelected)
                .fact(CapabilityKey::InstalledApps)
                .support,
            Support::Unknown
        );
    }
    #[tokio::test(start_paused = true)]
    async fn monotonic_expiry_ignores_wall_clock_and_duplicate_events() {
        let mut store = CapabilityStore::default();
        store.session(Some((DeviceId::new(1), context(1, 1))));
        let start = Instant::now();
        assert!(observe(
            &mut store,
            CapabilityKey::InstalledApps,
            EvidenceSource::TvResponse,
            100,
            start
        ));
        tokio::time::advance(Duration::from_secs(4)).await;
        assert!(!observe(
            &mut store,
            CapabilityKey::InstalledApps,
            EvidenceSource::TvResponse,
            1,
            Instant::now()
        ));
        assert!(!store.expire(Instant::now()));
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(store.expire(Instant::now()));
        assert!(!store.expire(Instant::now()));
        let summary = store.summary(DeviceId::new(1), CapabilityReason::Connected);
        assert_eq!(
            summary.reason_for(CapabilityKey::InstalledApps),
            CapabilityReason::EvidenceStale
        );
        assert_eq!(
            summary
                .fact(CapabilityKey::InstalledApps)
                .observed_at_unix_ms,
            Some(100)
        );
    }
    #[test]
    fn every_session_fence_retains_history_but_rejects_late_evidence_and_clears_ime() {
        for next in [
            None,
            Some((DeviceId::new(1), context(1, 2))),
            Some((DeviceId::new(1), context(2, 2))),
            Some((DeviceId::new(2), context(2, 2))),
            Some((
                DeviceId::new(1),
                EvidenceContext {
                    epoch: Epoch::from_u128(2),
                    ..context(1, 1)
                },
            )),
        ] {
            let mut store = CapabilityStore::default();
            store.session(Some((DeviceId::new(1), context(1, 1))));
            assert!(store.ime(DeviceId::new(1), context(1, 1), true, 100, Instant::now()));
            store.session(next);
            assert!(!store.ime(DeviceId::new(1), context(1, 1), true, 101, Instant::now()));
            let summary = store.summary(DeviceId::new(1), CapabilityReason::Disconnected);
            assert!(!summary.ime_active);
            assert_eq!(
                summary.fact(CapabilityKey::NativeIme).freshness,
                Freshness::Stale
            );
            assert_eq!(
                summary.fact(CapabilityKey::NativeIme).support,
                Support::Supported
            );
        }
    }
    #[test]
    fn unmatched_end_duplicate_start_invalid_clock_forget_and_restart() {
        let mut store = CapabilityStore::default();
        store.session(Some((DeviceId::new(1), context(1, 1))));
        assert!(!store.ime(DeviceId::new(1), context(1, 1), true, 0, Instant::now()));
        assert!(
            !store
                .summary(DeviceId::new(1), CapabilityReason::Connected)
                .ime_active
        );
        assert!(store.ime(DeviceId::new(1), context(1, 1), false, 10, Instant::now()));
        assert!(
            !store
                .summary(DeviceId::new(1), CapabilityReason::Connected)
                .ime_active
        );
        assert!(store.ime(DeviceId::new(1), context(1, 1), true, 11, Instant::now()));
        assert!(!store.ime(DeviceId::new(1), context(1, 1), true, 12, Instant::now()));
        store.session(None);
        store.retain(&[]);
        assert!(store.history.is_empty());
        assert_eq!(
            CapabilityStore::default()
                .summary(DeviceId::new(1), CapabilityReason::Disconnected)
                .fact(CapabilityKey::NativeIme)
                .support,
            Support::Unknown
        );
    }
    #[test]
    fn expiring_lifecycle_evidence_cannot_leave_an_active_field() {
        let mut store = CapabilityStore::default();
        let now = Instant::now();
        store.session(Some((DeviceId::new(1), context(1, 1))));
        assert!(store.observe(
            DeviceId::new(1),
            context(1, 1),
            CapabilityObservation {
                key: CapabilityKey::NativeIme,
                support: Support::Supported,
                source: EvidenceSource::TvEvent,
                wall_ms: 10,
                ttl: Some(Duration::from_secs(1)),
            },
            now
        ));
        assert!(store.ime(DeviceId::new(1), context(1, 1), true, 10, now));
        assert!(store.expire(now + Duration::from_secs(1)));
        assert!(
            !store
                .summary(DeviceId::new(1), CapabilityReason::Connected)
                .ime_active
        );
    }
    #[test]
    fn unknown_and_negative_evidence_invariants() {
        for key in CapabilityKey::ALL {
            let mut fact = CapabilityFact::unknown(key);
            assert!(fact.valid());
            fact.observed_at_unix_ms = Some(10);
            assert!(!fact.valid());
        }
        let mut store = CapabilityStore::default();
        store.session(Some((DeviceId::new(1), context(1, 1))));
        assert!(!store.observe(
            DeviceId::new(1),
            context(1, 1),
            CapabilityObservation {
                key: CapabilityKey::NativeIme,
                support: Support::Unsupported,
                source: EvidenceSource::TvEvent,
                wall_ms: 10,
                ttl: None,
            },
            Instant::now()
        ));
        assert!(store.observe(
            DeviceId::new(1),
            context(1, 1),
            CapabilityObservation {
                key: CapabilityKey::AppLaunch,
                support: Support::Unsupported,
                source: EvidenceSource::TvResponse,
                wall_ms: 10,
                ttl: None,
            },
            Instant::now()
        ));
        assert_eq!(
            store
                .summary(DeviceId::new(1), CapabilityReason::Connected)
                .reason_for(CapabilityKey::AppLaunch),
            CapabilityReason::FeatureUnsupported
        );
    }
}

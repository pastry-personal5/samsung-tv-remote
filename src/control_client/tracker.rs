//! One bounded registry for local requests and private retained references.
//! Tickets fence late HTTP replies; the registry never stores mutation bodies.
use super::{model::FailureCode, projection::NativeReceipt};
use crate::application::control_service::{Epoch, OperationId};
use std::collections::VecDeque;

pub(crate) const CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownReason {
    ChangedEpoch,
    OutcomeUnavailable,
    UnknownAfterRestart,
}

#[derive(Debug, Clone)]
pub enum Settlement {
    Local {
        tag: u64,
        receipt: NativeReceipt,
    },
    History(NativeReceipt),
    NotAdmitted {
        tag: u64,
        code: FailureCode,
    },
    KnownBeforeRestart {
        tag: u64,
        receipt: NativeReceipt,
    },
    Unknown {
        tag: u64,
        reason: UnknownReason,
    },
    HistoryUnavailable {
        epoch: Epoch,
        id: OperationId,
        reason: UnknownReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ticket(u64);

#[derive(Clone)]
pub(crate) struct Lookup {
    pub ticket: Ticket,
    pub epoch: Epoch,
    pub key: Option<String>,
    pub id: Option<OperationId>,
}
impl Lookup {
    pub fn path(&self) -> String {
        use crate::server::control_http::wire;
        match (self.id, &self.key) {
            (Some(id), _) => format!(
                "/v1/operations/{}?epoch={}",
                wire::operation(id),
                wire::epoch(self.epoch)
            ),
            (_, Some(key)) => format!(
                "/v1/operations/by-key/{key}?epoch={}",
                wire::epoch(self.epoch)
            ),
            _ => unreachable!("every tracker entry has a key or operation ID"),
        }
    }
}
#[derive(Clone)]
enum Ready {
    Receipt(NativeReceipt),
    Unknown(UnknownReason),
    Rejected(FailureCode),
}
pub(crate) struct Delivery {
    pub ticket: Ticket,
    pub version: u64,
    pub update: Settlement,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReserveError {
    StaleEpoch,
    Busy,
}
struct Entry {
    lookup: Lookup,
    tag: Option<u64>,
    in_flight: bool,
    announced: bool,
    ready: Option<Ready>,
    version: u64,
}

#[derive(Default)]
pub(crate) struct Tracker {
    epoch: Option<Epoch>,
    serial: u64,
    entries: Vec<Entry>,
    settled: VecDeque<(Epoch, OperationId)>,
}
impl Tracker {
    pub fn reserve(&mut self, epoch: Epoch, tag: u64, key: String) -> Result<Ticket, ReserveError> {
        if self.epoch.is_some_and(|current| current != epoch) {
            return Err(ReserveError::StaleEpoch);
        }
        if self.entries.iter().any(|entry| entry.tag == Some(tag)) {
            return Err(ReserveError::Busy);
        }
        // Reserve independent bounded shares inside the same registry: 64
        // local intents and 64 retained references. A user intent must neither
        // discard restored history nor be crowded out by that history.
        if self
            .entries
            .iter()
            .filter(|entry| entry.tag.is_some())
            .count()
            >= CAPACITY
        {
            return Err(ReserveError::Busy);
        }
        let ticket = self.ticket().ok_or(ReserveError::Busy)?;
        self.entries.push(Entry {
            lookup: Lookup {
                ticket,
                epoch,
                key: Some(key),
                id: None,
            },
            tag: Some(tag),
            in_flight: true,
            announced: false,
            ready: None,
            version: 0,
        });
        Ok(ticket)
    }
    fn ticket(&mut self) -> Option<Ticket> {
        self.serial = self.serial.checked_add(1)?;
        Some(Ticket(self.serial))
    }
    pub fn contains(&self, ticket: Ticket) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.lookup.ticket == ticket)
    }
    pub fn ambiguous(&mut self, ticket: Ticket) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.lookup.ticket == ticket)
        {
            entry.in_flight = false;
        }
    }
    pub fn not_admitted(&mut self, ticket: Ticket, code: FailureCode) -> bool {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.lookup.ticket == ticket)
        else {
            return false;
        };
        if entry.lookup.id.is_some() || matches!(entry.ready, Some(Ready::Unknown(_))) {
            return false;
        }
        entry.ready = Some(Ready::Rejected(code));
        entry.in_flight = false;
        entry.version = entry.version.saturating_add(1);
        true
    }
    pub fn attach(&mut self, epoch: Epoch, history: &[OperationId]) -> Vec<Settlement> {
        let mut updates = Vec::new();
        self.entries.retain(|entry| {
            if entry.lookup.epoch == epoch {
                // Rebuild read-only history from the new atomic snapshot;
                // preserve unresolved local requests and their original keys.
                return entry.tag.is_some();
            }
            if let Some(tag) = entry.tag {
                updates.push(match &entry.ready {
                    Some(Ready::Receipt(receipt)) if receipt.terminal => {
                        Settlement::KnownBeforeRestart {
                            tag,
                            receipt: receipt.clone(),
                        }
                    }
                    Some(Ready::Rejected(code)) => Settlement::NotAdmitted { tag, code: *code },
                    _ => Settlement::Unknown {
                        tag,
                        reason: UnknownReason::ChangedEpoch,
                    },
                });
            }
            false
        });
        self.settled.retain(|(current, _)| *current == epoch);
        self.epoch = Some(epoch);
        for id in history {
            self.reference(epoch, *id);
        }
        updates
    }
    /// False requests a new atomic subscription rather than silently losing
    /// a shared event reference when the bounded read-only share is full.
    pub fn reference(&mut self, epoch: Epoch, id: OperationId) -> bool {
        if self.epoch != Some(epoch) {
            return false;
        }
        if self.settled.contains(&(epoch, id))
            || self
                .entries
                .iter()
                .any(|entry| entry.lookup.epoch == epoch && entry.lookup.id == Some(id))
        {
            return true;
        }
        if self
            .entries
            .iter()
            .filter(|entry| entry.tag.is_none())
            .count()
            >= CAPACITY
        {
            return false;
        }
        let Some(ticket) = self.ticket() else {
            return false;
        };
        self.entries.push(Entry {
            lookup: Lookup {
                ticket,
                epoch,
                key: None,
                id: Some(id),
            },
            tag: None,
            in_flight: false,
            announced: false,
            ready: None,
            version: 0,
        });
        true
    }
    pub fn lookups(&self) -> Vec<Lookup> {
        // Resolve keys before private references so a local operation cannot be
        // mistaken for restored history when its admission reply was lost.
        let unresolved_local = self
            .entries
            .iter()
            .any(|entry| entry.tag.is_some() && entry.lookup.id.is_none());
        self.entries
            .iter()
            .filter(|entry| {
                !entry.in_flight
                    && entry.ready.is_none()
                    && (!unresolved_local || entry.tag.is_some())
            })
            .map(|entry| entry.lookup.clone())
            .collect()
    }
    fn remember(&mut self, epoch: Epoch, id: OperationId) {
        if !self.settled.contains(&(epoch, id)) {
            if self.settled.len() == CAPACITY {
                self.settled.pop_front();
            }
            self.settled.push_back((epoch, id));
        }
    }
    /// Merge facts without consuming their delivery. Publication can be
    /// cancelled by resubscription, so retirement happens only after ack().
    pub fn receipt(&mut self, ticket: Ticket, receipt: &NativeReceipt) -> bool {
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.lookup.ticket == ticket)
        else {
            return false;
        };
        let entry = &self.entries[index];
        if entry.lookup.epoch != receipt.epoch || entry.lookup.id.is_some_and(|id| id != receipt.id)
        {
            return false;
        }
        if matches!(&entry.ready, Some(Ready::Receipt(previous)) if previous.terminal)
            || matches!(entry.ready, Some(Ready::Unknown(_) | Ready::Rejected(_)))
        {
            return true;
        }
        let tag = entry.tag;
        if tag.is_some() {
            self.entries.retain(|entry| {
                entry.tag.is_some()
                    || entry.lookup.epoch != receipt.epoch
                    || entry.lookup.id != Some(receipt.id)
            });
        }
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.lookup.ticket == ticket)
            .expect("retained local association");
        entry.lookup.id = Some(receipt.id);
        entry.in_flight = false;
        if receipt.terminal || tag.is_some() && !entry.announced {
            entry.ready = Some(Ready::Receipt(receipt.clone()));
            entry.version = entry.version.saturating_add(1);
        }
        true
    }
    pub fn unavailable(&mut self, ticket: Ticket, reason: UnknownReason) {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.lookup.ticket == ticket)
        else {
            return;
        };
        // Already captured terminal facts outrank a later retention response.
        if matches!(&entry.ready, Some(Ready::Receipt(receipt)) if receipt.terminal) {
            return;
        }
        entry.ready = Some(Ready::Unknown(reason));
        entry.in_flight = false;
        entry.version = entry.version.saturating_add(1);
    }
    pub fn deliveries(&self) -> Vec<Delivery> {
        self.entries
            .iter()
            .filter_map(|entry| {
                let update = match (&entry.ready, entry.tag) {
                    (Some(Ready::Receipt(receipt)), Some(tag)) => Settlement::Local {
                        tag,
                        receipt: receipt.clone(),
                    },
                    (Some(Ready::Receipt(receipt)), None) => Settlement::History(receipt.clone()),
                    (Some(Ready::Unknown(reason)), Some(tag)) => Settlement::Unknown {
                        tag,
                        reason: *reason,
                    },
                    (Some(Ready::Unknown(reason)), None) => Settlement::HistoryUnavailable {
                        epoch: entry.lookup.epoch,
                        id: entry.lookup.id.expect("history ID"),
                        reason: *reason,
                    },
                    (Some(Ready::Rejected(code)), Some(tag)) => {
                        Settlement::NotAdmitted { tag, code: *code }
                    }
                    (Some(Ready::Rejected(_)), None) => {
                        unreachable!("only local requests can be not admitted")
                    }
                    (None, _) => return None,
                };
                Some(Delivery {
                    ticket: entry.lookup.ticket,
                    version: entry.version,
                    update,
                })
            })
            .collect()
    }
    pub fn ack(&mut self, ticket: Ticket, version: u64) {
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.lookup.ticket == ticket && entry.version == version)
        else {
            return;
        };
        let entry = &mut self.entries[index];
        let terminal = match entry.ready.take() {
            Some(Ready::Receipt(receipt)) => receipt.terminal,
            Some(Ready::Unknown(_) | Ready::Rejected(_)) => true,
            None => return,
        };
        entry.announced = true;
        if terminal {
            let entry = self.entries.remove(index);
            if let Some(id) = entry.lookup.id {
                self.remember(entry.lookup.epoch, id);
            }
        }
    }
    pub fn unowned_reference(&mut self, ticket: Ticket) {
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.lookup.ticket == ticket && entry.tag.is_none())
        else {
            return;
        };
        let entry = self.entries.remove(index);
        self.remember(entry.lookup.epoch, entry.lookup.id.expect("history ID"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_client::projection::NativeOutcome;
    fn receipt(epoch: Epoch, id: u64, terminal: bool) -> NativeReceipt {
        NativeReceipt {
            id: OperationId(id),
            epoch,
            target: None,
            generation: None,
            terminal,
            outcome: terminal.then_some(NativeOutcome::Written),
            expires_at: terminal.then(|| std::num::NonZeroU64::new(1).unwrap()),
            terminal_state: terminal.then_some(
                crate::application::control_service::TerminalState {
                    selected: None,
                    generation: 0,
                    configuration_revision: None,
                },
            ),
        }
    }
    fn deliver(tracker: &mut Tracker) -> Vec<Settlement> {
        tracker
            .deliveries()
            .into_iter()
            .map(|delivery| {
                tracker.ack(delivery.ticket, delivery.version);
                delivery.update
            })
            .collect()
    }
    #[test]
    fn lost_admission_is_correlated_before_history_and_late_receipts_never_reopen_it() {
        let epoch = Epoch::from_u128(1);
        let mut tracker = Tracker::default();
        tracker.attach(epoch, &[]);
        let local = tracker.reserve(epoch, 10, "original-key".into()).unwrap();
        tracker.reference(epoch, OperationId(5));
        assert!(tracker.lookups().is_empty());
        tracker.ambiguous(local);
        let jobs = tracker.lookups();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].key.as_deref(), Some("original-key"));
        assert!(tracker.receipt(local, &receipt(epoch, 5, true)));
        assert!(matches!(
            deliver(&mut tracker).as_slice(),
            [Settlement::Local { tag: 10, .. }]
        ));
        assert!(tracker.entries.is_empty());
        assert!(!tracker.receipt(local, &receipt(epoch, 5, false)));
        tracker.reference(epoch, OperationId(5));
        assert!(tracker.lookups().is_empty());
    }
    #[test]
    fn known_terminal_before_restart_is_history_without_a_workflow_continuation() {
        let old = Epoch::from_u128(1);
        let mut tracker = Tracker::default();
        tracker.attach(old, &[]);
        let local = tracker.reserve(old, 7, "key".into()).unwrap();
        tracker.receipt(local, &receipt(old, 3, true));
        assert!(matches!(
            tracker.attach(Epoch::from_u128(2), &[]).as_slice(),
            [Settlement::KnownBeforeRestart {
                tag: 7,
                receipt: NativeReceipt { terminal: true, .. }
            }]
        ));
        assert!(tracker.entries.is_empty());
    }
    #[test]
    fn cancelled_rejection_delivery_keeps_its_precise_nonadmission_fact() {
        let epoch = Epoch::from_u128(1);
        let mut tracker = Tracker::default();
        tracker.attach(epoch, &[]);
        let local = tracker.reserve(epoch, 1, "key".into()).unwrap();
        assert!(tracker.not_admitted(local, FailureCode::StaleEpoch));
        assert!(tracker.lookups().is_empty());
        assert_eq!(tracker.deliveries().len(), 1);
        assert!(matches!(
            deliver(&mut tracker).as_slice(),
            [Settlement::NotAdmitted {
                tag: 1,
                code: FailureCode::StaleEpoch
            }]
        ));
        assert!(!tracker.contains(local));
    }
    #[test]
    fn pending_then_terminal_is_monotonic_and_history_has_no_local_tag() {
        let epoch = Epoch::from_u128(1);
        let mut tracker = Tracker::default();
        tracker.attach(epoch, &[OperationId(8)]);
        let history = tracker.lookups()[0].ticket;
        assert!(tracker.receipt(history, &receipt(epoch, 8, false)));
        assert!(tracker.deliveries().is_empty());
        assert!(tracker.receipt(history, &receipt(epoch, 8, true)));
        assert!(matches!(
            deliver(&mut tracker).as_slice(),
            [Settlement::History(_)]
        ));
        let local = tracker.reserve(epoch, 3, "key".into()).unwrap();
        tracker.ambiguous(local);
        tracker.receipt(local, &receipt(epoch, 9, false));
        assert!(matches!(
            deliver(&mut tracker).as_slice(),
            [Settlement::Local { tag: 3, .. }]
        ));
        tracker.receipt(local, &receipt(epoch, 9, false));
        assert!(tracker.deliveries().is_empty());
        tracker.receipt(local, &receipt(epoch, 9, true));
        assert!(matches!(
            deliver(&mut tracker).as_slice(),
            [Settlement::Local { tag: 3, .. }]
        ));
        assert!(!tracker.receipt(local, &receipt(epoch, 9, false)));
    }
    #[test]
    fn cancelled_publication_keeps_terminal_facts_and_stale_pending_ack_cannot_consume_them() {
        let epoch = Epoch::from_u128(1);
        let mut tracker = Tracker::default();
        tracker.attach(epoch, &[]);
        let local = tracker.reserve(epoch, 1, "key".into()).unwrap();
        tracker.receipt(local, &receipt(epoch, 4, false));
        let pending = tracker.deliveries().remove(0);
        tracker.receipt(local, &receipt(epoch, 4, true));
        tracker.ack(pending.ticket, pending.version); // late pending delivery
        assert!(tracker.contains(local));
        assert!(tracker.lookups().is_empty());
        let terminal = tracker.deliveries().remove(0); // emulate send cancelled before ack
        assert!(matches!(
            terminal.update,
            Settlement::Local {
                receipt: NativeReceipt { terminal: true, .. },
                ..
            }
        ));
        tracker.receipt(local, &receipt(epoch, 4, false)); // pending cannot overwrite
        assert!(matches!(
            deliver(&mut tracker).as_slice(),
            [Settlement::Local {
                receipt: NativeReceipt { terminal: true, .. },
                ..
            }]
        ));
        assert!(!tracker.contains(local));
    }
    #[test]
    fn epoch_change_settles_local_uncertainty_once_and_fences_old_jobs() {
        let old = Epoch::from_u128(1);
        let new = Epoch::from_u128(2);
        let mut tracker = Tracker::default();
        tracker.attach(old, &[OperationId(7)]);
        let history = tracker.lookups()[0].ticket;
        let local = tracker.reserve(old, 4, "key".into()).unwrap();
        assert!(tracker.attach(old, &[OperationId(7)]).is_empty());
        assert!(matches!(
            tracker.attach(new, &[OperationId(7)]).as_slice(),
            [Settlement::Unknown {
                tag: 4,
                reason: UnknownReason::ChangedEpoch
            }]
        ));
        assert!(!tracker.receipt(local, &receipt(old, 7, true)));
        assert!(!tracker.receipt(history, &receipt(old, 7, true)));
        assert!(tracker.attach(new, &[OperationId(7)]).is_empty());
        assert_eq!(tracker.lookups()[0].epoch, new);
        assert_eq!(
            tracker.reserve(old, 5, "stale".into()),
            Err(ReserveError::StaleEpoch)
        );
    }
    #[test]
    fn missing_private_reference_does_not_settle_local_keys_and_both_shares_are_bounded() {
        let epoch = Epoch::from_u128(1);
        let mut tracker = Tracker::default();
        let refs: Vec<_> = (1..=64).map(OperationId).collect();
        tracker.attach(epoch, &refs);
        assert_eq!(tracker.lookups().len(), CAPACITY);
        let local = tracker.reserve(epoch, 1, "key".into()).unwrap();
        assert_eq!(tracker.entries.len(), CAPACITY + 1);
        tracker.unowned_reference(local);
        assert!(tracker.contains(local));
        tracker.ambiguous(local);
        assert_eq!(tracker.lookups().len(), 1);
        tracker.unavailable(local, UnknownReason::OutcomeUnavailable);
        assert!(matches!(
            deliver(&mut tracker).as_slice(),
            [Settlement::Unknown { tag: 1, .. }]
        ));
        let history = tracker.lookups()[0].ticket;
        tracker.unowned_reference(history);
        assert!(!tracker.contains(history));
        tracker.entries.clear();
        for tag in 0..64 {
            assert!(tracker.reserve(epoch, tag, format!("key-{tag}")).is_ok());
        }
        assert_eq!(
            tracker.reserve(epoch, 65, "overflow".into()),
            Err(ReserveError::Busy)
        );
        for id in 100..164 {
            assert!(tracker.reference(epoch, OperationId(id)));
        }
        assert_eq!(tracker.entries.len(), CAPACITY * 2);
        assert!(!tracker.reference(epoch, OperationId(200)));
    }
    #[test]
    fn wrong_lookup_identity_cannot_replace_captured_operation() {
        let epoch = Epoch::from_u128(1);
        let mut tracker = Tracker::default();
        tracker.attach(epoch, &[]);
        let local = tracker.reserve(epoch, 2, "key".into()).unwrap();
        tracker.receipt(local, &receipt(epoch, 4, false));
        deliver(&mut tracker);
        assert!(!tracker.receipt(local, &receipt(epoch, 5, true)));
        assert!(!tracker.receipt(local, &receipt(Epoch::from_u128(2), 4, true)));
        assert!(tracker.contains(local));
        assert_eq!(tracker.lookups()[0].id, Some(OperationId(4)));
    }
    #[test]
    fn saturated_references_require_resnapshot_without_discarding_local_intents() {
        let epoch = Epoch::from_u128(1);
        let mut tracker = Tracker::default();
        tracker.attach(epoch, &[]);
        for id in 1..=64 {
            assert!(tracker.reference(epoch, OperationId(id)));
        }
        assert!(!tracker.reference(epoch, OperationId(65)));
        let local = tracker.reserve(epoch, 1, "original".into()).unwrap();
        tracker.ambiguous(local);
        tracker.attach(epoch, &[OperationId(65)]);
        assert!(tracker.contains(local));
        assert_eq!(tracker.entries.len(), 2);
        tracker.not_admitted(local, FailureCode::Busy);
        deliver(&mut tracker);
        assert_eq!(tracker.lookups()[0].id, Some(OperationId(65)));
    }
}

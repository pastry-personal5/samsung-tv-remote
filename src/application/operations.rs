use super::control_service::{ClientId, Command, OperationId, Outcome, Receipt, ServiceError};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

// A thin shared handle keeps receipts small enough for inline owner messages.
// Borrow<str> preserves indexed lookup without allocating a query key.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Key(Arc<String>);
impl std::borrow::Borrow<str> for Key {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

struct Record {
    client: ClientId,
    key: Key,
    command: Command,
    receipt: Receipt,
    terminal: bool,
}
/// Both lookup directions are indexed. Expiry processes only expired results,
/// rather than scanning a saturated registry on every owner transition.
pub(crate) struct Operations {
    records: HashMap<OperationId, Record>,
    epoch: super::control_service::Epoch,
    keys: HashMap<ClientId, HashMap<Key, OperationId>>,
    pending: HashMap<ClientId, usize>,
    expiry: VecDeque<(Instant, OperationId)>,
    next: u64,
    capacity: usize,
    retention: Duration,
}
impl Operations {
    pub fn new(epoch: super::control_service::Epoch, capacity: usize, retention: Duration) -> Self {
        Self {
            records: HashMap::new(),
            epoch,
            keys: HashMap::new(),
            pending: HashMap::new(),
            expiry: VecDeque::new(),
            next: 1,
            capacity,
            retention,
        }
    }
    pub fn expire(&mut self) {
        let now = Instant::now();
        while self
            .expiry
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) >= self.retention)
        {
            let (_, id) = self.expiry.pop_front().expect("expiry front");
            if let Some(record) = self.records.remove(&id) {
                let keys = self.keys.get_mut(&record.client).expect("client index");
                keys.remove(&record.key);
                if keys.is_empty() {
                    self.keys.remove(&record.client);
                    self.pending.remove(&record.client);
                }
            }
        }
    }
    pub fn lookup(&self, client: ClientId, key: &str) -> Option<Receipt> {
        let id = self.keys.get(&client)?.get(key)?;
        Some(self.records.get(id)?.receipt.clone())
    }
    pub fn operation(&self, client: ClientId, id: OperationId) -> Option<Receipt> {
        self.records
            .get(&id)
            .filter(|r| r.client == client)
            .map(|r| r.receipt.clone())
    }
    /// Newest 64 retained references only; never include another identity's
    /// receipts in shared events or perform a second query after a snapshot.
    pub fn recent(&self, client: ClientId) -> Vec<OperationId> {
        let mut ids: Vec<_> = self
            .keys
            .get(&client)
            .into_iter()
            .flat_map(|keys| keys.values().copied())
            .collect();
        ids.sort_unstable_by_key(|id| std::cmp::Reverse(id.0));
        ids.truncate(64);
        ids
    }
    pub fn duplicate(
        &self,
        client: ClientId,
        key: &str,
        command: &Command,
    ) -> Result<Option<Receipt>, ServiceError> {
        let Some(id) = self.keys.get(&client).and_then(|keys| keys.get(key)) else {
            return Ok(None);
        };
        let record = self.records.get(id).expect("operation index");
        if &record.command == command {
            Ok(Some(record.receipt.clone()))
        } else {
            Err(ServiceError::KeyConflict)
        }
    }
    pub fn admit(
        &mut self,
        client: ClientId,
        key: String,
        command: Command,
    ) -> Result<(Receipt, bool), ServiceError> {
        if let Some(id) = self
            .keys
            .get(&client)
            .and_then(|keys| keys.get(key.as_str()))
        {
            let record = self.records.get(id).expect("operation index");
            return if record.command == command {
                Ok((record.receipt.clone(), false))
            } else {
                Err(ServiceError::KeyConflict)
            };
        }
        if self.records.len() == self.capacity {
            return Err(ServiceError::RegistryFull);
        }
        if self
            .keys
            .get(&client)
            .is_some_and(|keys| keys.len() >= 8192)
            || self.pending.get(&client).is_some_and(|count| *count >= 64)
        {
            return Err(ServiceError::ClientLimit);
        }
        let key = Key(Arc::new(key));
        let receipt = Receipt {
            id: OperationId(self.next),
            key: key.0.clone(),
            metadata: command.metadata(self.epoch),
            outcome: None,
            expires_at: None,
            terminal_state: None,
        };
        self.next = self
            .next
            .checked_add(1)
            .expect("operation identity exhausted");
        self.keys
            .entry(client)
            .or_default()
            .insert(key.clone(), receipt.id);
        *self.pending.entry(client).or_default() += 1;
        self.records.insert(
            receipt.id,
            Record {
                client,
                key,
                command,
                receipt: receipt.clone(),
                terminal: false,
            },
        );
        Ok((receipt, true))
    }
    pub fn finish(
        &mut self,
        id: OperationId,
        outcome: Outcome,
        state: Option<super::control_service::TerminalState>,
    ) -> bool {
        let Some(record) = self.records.get_mut(&id) else {
            return false;
        };
        if record.terminal {
            return false;
        }
        record.receipt.outcome = Some(outcome);
        record.receipt.terminal_state = state.map(Box::new);
        record.receipt.expires_at = Some(super::control_service::display_deadline(self.retention));
        record.terminal = true;
        *self.pending.get_mut(&record.client).expect("pending count") -= 1;
        self.expiry.push_back((Instant::now(), id));
        true
    }
    pub fn phase(&self, id: OperationId) -> Option<super::control_service::OperationPhase> {
        self.records.get(&id).map(|r| {
            if r.terminal {
                super::control_service::OperationPhase::Terminal
            } else {
                super::control_service::OperationPhase::Pending
            }
        })
    }
    pub fn pending_ids(&self) -> Vec<OperationId> {
        self.records
            .iter()
            .filter(|(_, r)| !r.terminal)
            .map(|(id, _)| *id)
            .collect()
    }
    pub fn target(&self, id: OperationId) -> Option<crate::domain::DeviceId> {
        self.records.get(&id)?.receipt.metadata.target
    }
}

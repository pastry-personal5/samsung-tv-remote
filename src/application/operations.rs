use super::control_service::{ClientId, Command, OperationId, Outcome, Receipt, ServiceError};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

struct Record {
    client: ClientId,
    key: Arc<str>,
    command: Command,
    receipt: Receipt,
    terminal: bool,
}
/// Both lookup directions are indexed. Expiry processes only expired results,
/// rather than scanning a saturated registry on every owner transition.
pub(crate) struct Operations {
    records: HashMap<OperationId, Record>,
    epoch: super::control_service::Epoch,
    keys: HashMap<ClientId, HashMap<Arc<str>, OperationId>>,
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
        let receipt = Receipt {
            id: OperationId(self.next),
            metadata: command.metadata(self.epoch),
            outcome: None,
        };
        self.next = self
            .next
            .checked_add(1)
            .expect("operation identity exhausted");
        let key: Arc<str> = key.into();
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
    pub fn finish(&mut self, id: OperationId, outcome: Outcome) -> bool {
        let Some(record) = self.records.get_mut(&id) else {
            return false;
        };
        if record.terminal {
            return false;
        }
        record.receipt.outcome = Some(outcome);
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
    pub fn drain(&mut self) {
        let ids: Vec<_> = self
            .records
            .iter()
            .filter(|(_, r)| !r.terminal)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.finish(id, Outcome::Drained);
        }
    }
}

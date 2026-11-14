//! Typed commands enter the same service and strict decoder as /v1.
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::time::Instant;

use super::{
    catalog,
    cursor::{self, Scope},
    results, Adapter,
};
use crate::application::control_service::*;
use crate::application::control_state::{ConnectionState, PairingState};
use crate::server::access::AuthorizedClient;
use crate::server::control_http::{
    request::{self, MutationRoute},
    response, transport, wire,
};

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct PageArgs {
    page_size: Option<usize>,
    cursor: Option<String>,
}
impl PageArgs {
    fn size(&self) -> Result<usize, &'static str> {
        let size = self.page_size.unwrap_or(catalog::PAGE_DEFAULT);
        if !(1..=catalog::PAGE_MAX).contains(&size) {
            Err("invalid_request")
        } else {
            Ok(size)
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inspect {
    device_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationArgs {
    server_epoch: String,
    operation_id: Option<String>,
    operation_key: Option<String>,
    wait_seconds: Option<u64>,
    page_size: Option<usize>,
    cursor: Option<String>,
}
enum Selector {
    Key(String),
    Id(OperationId),
}
impl Selector {
    async fn lookup(&self, service: &ControlService, epoch: Epoch) -> Result<Lookup, ServiceError> {
        match self {
            Self::Key(key) => service.lookup(epoch, key.clone()).await,
            Self::Id(id) => service.operation(epoch, *id).await,
        }
    }
}

fn key(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

/// Check the primitive advertised schema constraints before typed decoding.
/// Cross-field confirmation and application policy remain in the shared DTO/service.
pub(super) fn validate(name: &str, arguments: &Value) -> Result<(), &'static str> {
    let schema = &catalog::tools()
        .as_array()
        .expect("catalog")
        .iter()
        .find(|tool| tool["name"] == name)
        .ok_or("invalid_request")?["inputSchema"];
    let properties = schema["properties"].as_object().expect("schema properties");
    let fields = arguments.as_object().ok_or("invalid_request")?;
    for required in schema["required"].as_array().expect("required fields") {
        if !fields.contains_key(required.as_str().expect("field")) {
            return Err("invalid_request");
        }
    }
    for (field, value) in fields {
        let property = properties.get(field).ok_or("invalid_request")?;
        if value.is_null() {
            if matches!(
                field.as_str(),
                "wired_mac" | "wifi_mac" | "active_interface"
            ) {
                continue;
            }
            return Err("invalid_request");
        }
        if let Some(allowed) = property.get("enum").and_then(Value::as_array) {
            if !allowed.contains(value) {
                return Err("invalid_request");
            }
        }
        if matches!(field.as_str(), "page_size" | "wait_seconds") {
            let n = value.as_u64().ok_or("invalid_request")?;
            if n < property["minimum"].as_u64().expect("minimum")
                || n > property["maximum"].as_u64().expect("maximum")
            {
                return Err("invalid_request");
            }
            continue;
        }
        let value = value.as_str().ok_or("invalid_request")?;
        if property["format"] == "uuid" && !key(value) {
            return Err("invalid_id");
        }
        if matches!(
            field.as_str(),
            "expected_generation" | "selection_generation" | "expected_config_revision"
        ) && serde_json::from_value::<wire::Counter>(json!(value)).is_err()
        {
            return Err("invalid_request");
        }
        if matches!(field.as_str(), "fingerprint" | "confirm_fingerprint")
            && (value.len() != 64
                || !value
                    .bytes()
                    .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v)))
        {
            return Err("invalid_confirmation");
        }
        if matches!(field.as_str(), "wired_mac" | "wifi_mac") {
            let parts: Vec<_> = value.split(':').collect();
            if parts.len() != 6
                || parts
                    .iter()
                    .any(|p| p.len() != 2 || !p.bytes().all(|v| v.is_ascii_hexdigit()))
            {
                return Err("invalid_target");
            }
        }
        let count = value.chars().count();
        if property["minLength"]
            .as_u64()
            .is_some_and(|min| count < min as usize)
            || property["maxLength"]
                .as_u64()
                .is_some_and(|max| count > max as usize)
        {
            return Err("input_too_large");
        }
    }
    Ok(())
}

pub(super) async fn call(
    adapter: &Adapter,
    name: &str,
    arguments: Value,
    service: &ControlService,
    client: &AuthorizedClient,
    deadline: Instant,
) -> Value {
    if let Err(code) = validate(name, &arguments) {
        return results::failure(code, None);
    }
    let snapshot = match service.snapshot().await {
        Ok(snapshot) => snapshot,
        Err(error) => return results::failure(response::service_error(error).code, None),
    };
    match name {
        "list_tvs" => list(adapter, arguments, &snapshot, client.identity),
        "inspect_tv" => inspect(arguments, &snapshot),
        "get_operation" => {
            operation(adapter, arguments, service, client, &snapshot, deadline).await
        }
        _ => mutation(adapter, name, arguments, service, client, &snapshot).await,
    }
}

fn pairing(snapshot: &Snapshot) -> &'static str {
    match snapshot.control.pairing_state() {
        PairingState::NotStarted => "not_started",
        PairingState::InProgress => "in_progress",
        PairingState::Ready => "ready",
        PairingState::Failed => "failed",
    }
}
fn connection(snapshot: &Snapshot) -> &'static str {
    match snapshot.control.connection_state() {
        ConnectionState::NotConnected => "not_connected",
        ConnectionState::Connecting => "connecting",
        ConnectionState::Ready => "ready",
        ConnectionState::Failed => "failed",
    }
}
fn config_revision(snapshot: &Snapshot, id: crate::domain::DeviceId) -> u64 {
    snapshot
        .configuration_revisions
        .iter()
        .find(|(device, _)| *device == id)
        .map_or(0, |(_, r)| *r)
}
fn list(adapter: &Adapter, arguments: Value, snapshot: &Snapshot, client: ClientId) -> Value {
    let Ok(page) = serde_json::from_value::<PageArgs>(arguments) else {
        return results::failure("invalid_request", Some(snapshot));
    };
    let Ok(size) = page.size() else {
        return results::failure("invalid_request", Some(snapshot));
    };
    let mut devices: Vec<_> = snapshot.devices.iter().collect();
    devices.sort_unstable_by_key(|device| wire::device(device.id));
    let mut digest = Sha256::new();
    for device in &devices {
        let bytes = serde_json::to_vec(&response::device(
            device,
            config_revision(snapshot, device.id),
        ))
        .expect("device JSON");
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    let source = crate::server::bootstrap::hex(&digest.finalize().into());
    let scope = Scope {
        client,
        epoch: snapshot.epoch,
        query: "list_tvs",
        source: &source,
        size,
    };
    let offset = match adapter.cursors.decode(page.cursor.as_deref(), &scope) {
        Ok(v) if v <= devices.len() => v,
        Ok(_) => return results::failure("stale_cursor", Some(snapshot)),
        Err(code) => return results::failure(code, Some(snapshot)),
    };
    let tvs: Vec<_> = devices.iter().skip(offset).take(size).map(|device| {
        let (label, truncated) = results::preview(&device.label);
        json!({"device_id":wire::device(device.id),"label":label,"label_truncated":truncated,"config_revision":wire::Counter(config_revision(snapshot, device.id))})
    }).collect();
    let next = offset + tvs.len();
    let mut value = results::current(snapshot);
    let fields = value.as_object_mut().expect("context");
    fields.extend(json!({"revision":wire::Counter(snapshot.revision),"startup":response::startup(snapshot.startup),"admission_closed":snapshot.admission_closed,
        "recovery_required":snapshot.recovery.is_some() || snapshot.pairing_recovery.is_some(),"recovery_targets":recovery_targets(snapshot),"storage_pending":snapshot.storage_pending,"duplicate_records":snapshot.duplicate_records,
        "pairing":pairing(snapshot),"connection":connection(snapshot),"wake":response::wake(snapshot.wake),"tvs":tvs,
        "next_cursor":(next < devices.len()).then(|| adapter.cursors.encode(next, &scope))}).as_object().expect("list fields").clone());
    value
}
fn inspect(arguments: Value, snapshot: &Snapshot) -> Value {
    let Ok(args) = serde_json::from_value::<Inspect>(arguments) else {
        return results::failure("invalid_request", Some(snapshot));
    };
    let Ok(id) = wire::parse_device(&args.device_id) else {
        return results::failure("invalid_id", Some(snapshot));
    };
    let device = snapshot.devices.iter().find(|device| device.id == id);
    let has_recovery = snapshot.recovery.is_some_and(|marker| marker.target == id)
        || snapshot
            .pairing_recovery
            .is_some_and(|marker| marker.target == id);
    if device.is_none() && !has_recovery {
        return results::failure(
            if matches!(
                snapshot.startup,
                StartupState::Ready | StartupState::RecoveryRequired
            ) {
                "not_found"
            } else {
                "storage_blocked"
            },
            Some(snapshot),
        );
    }
    let tv = device.map(|device| {
        let mut tv = response::device(device, config_revision(snapshot, id));
        let (label, truncated) = results::preview(&device.label);
        tv["label"] = json!(label);
        tv["label_truncated"] = json!(truncated);
        tv
    });
    let mut value = results::current(snapshot);
    let facts = response::recovery(snapshot);
    let selected = snapshot.control.selected_device() == Some(id);
    value.as_object_mut().expect("context").extend(json!({"revision":wire::Counter(snapshot.revision),"startup":response::startup(snapshot.startup),"admission_closed":snapshot.admission_closed,
        "tv":tv,"connection":if selected { connection(snapshot) } else { "not_selected" },"pairing":if selected { pairing(snapshot) } else { "not_selected" },"wake":selected.then(|| response::wake(snapshot.wake)),
        "forget_recovery":snapshot.recovery.filter(|m|m.target==id).map(|_| &facts["forget_recovery"]),"pairing_recovery":snapshot.pairing_recovery.filter(|m|m.target==id).map(|_| &facts["pairing_recovery"])}).as_object().expect("detail fields").clone());
    value
}
fn recovery_targets(snapshot: &Snapshot) -> Vec<Value> {
    let mut targets = Vec::new();
    if let Some(marker) = snapshot.recovery {
        targets.push(json!({"device_id":wire::device(marker.target),"kind":"forget"}));
    }
    if let Some(marker) = snapshot.pairing_recovery {
        targets.push(json!({"device_id":wire::device(marker.target),"kind":if marker.repair {"re_pair"} else {"first_pair"}}));
    }
    targets
}

async fn operation(
    adapter: &Adapter,
    arguments: Value,
    service: &ControlService,
    client: &AuthorizedClient,
    snapshot: &Snapshot,
    deadline: Instant,
) -> Value {
    let Ok(args) = serde_json::from_value::<OperationArgs>(arguments) else {
        return results::failure("invalid_request", Some(snapshot));
    };
    let Ok(epoch) = wire::parse_epoch(&args.server_epoch) else {
        return results::failure("invalid_id", Some(snapshot));
    };
    let wait = args.wait_seconds.unwrap_or(0);
    let page = PageArgs {
        page_size: args.page_size,
        cursor: args.cursor,
    };
    let Ok(size) = page.size() else {
        return results::failure("invalid_request", Some(snapshot));
    };
    if wait > 10 || (wait > 0 && page.cursor.is_some()) {
        return results::failure("invalid_request", Some(snapshot));
    }
    if args.operation_id.is_some() == args.operation_key.is_some() {
        return results::failure("invalid_request", Some(snapshot));
    }
    if epoch != snapshot.epoch {
        return unknown("unknown_after_restart", &args.server_epoch, Some(snapshot));
    }
    let selector = match (args.operation_id, args.operation_key) {
        (Some(id), None) => match wire::parse_operation(&id, epoch, snapshot.epoch) {
            Ok(id) => Selector::Id(id),
            Err(_) => return results::failure("invalid_id", Some(snapshot)),
        },
        (None, Some(key)) if key.len() <= 128 => Selector::Key(key),
        _ => return results::failure("invalid_request", Some(snapshot)),
    };
    let lookup = match selector.lookup(service, epoch).await {
        Ok(v) => v,
        Err(error) => return results::failure(response::service_error(error).code, Some(snapshot)),
    };
    let pending = matches!(&lookup, Lookup::Known(receipt) if receipt.outcome.is_none());
    let lookup = if wait > 0 && pending {
        let Some(per) = adapter.waiters_by_client.get(&client.identity) else {
            return results::failure("unauthenticated", Some(snapshot));
        };
        let Ok(_global) = adapter.waiters.try_acquire() else {
            return results::failure("busy", Some(snapshot));
        };
        let Ok(_per) = per.try_acquire() else {
            return results::failure("busy", Some(snapshot));
        };
        let mut subscription = match service.subscribe().await {
            Ok(v) => v,
            Err(error) => {
                return results::failure(response::service_error(error).code, Some(snapshot))
            }
        };
        let end =
            (Instant::now() + Duration::from_secs(wait)).min(deadline - Duration::from_millis(250));
        let mut revoked = client.revoked.clone();
        loop {
            let current = match selector.lookup(service, epoch).await {
                Ok(v) => v,
                Err(error) => {
                    return results::failure(response::service_error(error).code, Some(snapshot))
                }
            };
            if !matches!(&current, Lookup::Known(receipt) if receipt.outcome.is_none())
                || Instant::now() >= end
            {
                break current;
            }
            let closed = tokio::select! { biased;
                _ = revoked.changed() => return results::failure("unauthenticated", None),
                event = subscription.events.recv() => event.is_none(),
                _ = tokio::time::sleep_until(end) => false,
            };
            if closed {
                // The owner closes a full subscriber; resubscribe. Only a
                // refused subscription means the service is stopping.
                if let Ok(next) = service.subscribe().await {
                    subscription = next;
                    continue;
                }
                let final_lookup = match selector.lookup(service, epoch).await {
                    Ok(v) => v,
                    Err(error) => {
                        return results::failure(
                            response::service_error(error).code,
                            Some(snapshot),
                        )
                    }
                };
                if let Lookup::Known(receipt) = &final_lookup {
                    if receipt.outcome.is_none() {
                        let mut value = results::receipt(receipt);
                        value["error"] =
                            results::failure("server_stopping", Some(snapshot))["error"].clone();
                        value["next_cursor"] = Value::Null;
                        return value;
                    }
                }
                break final_lookup;
            }
        }
    } else {
        lookup
    };
    match lookup {
        Lookup::Known(receipt) => project_receipt(adapter, &receipt, client.identity, &page, size),
        Lookup::UnknownAfterRestart => {
            unknown("unknown_after_restart", &args.server_epoch, Some(snapshot))
        }
        Lookup::OutcomeUnavailable => {
            unknown("outcome_unavailable", &args.server_epoch, Some(snapshot))
        }
    }
}
fn unknown(code: &'static str, epoch: &str, snapshot: Option<&Snapshot>) -> Value {
    let mut value = results::failure(code, snapshot);
    value["state"] = json!("unknown");
    value["server_epoch"] = json!(epoch);
    value
}
fn project_receipt(
    adapter: &Adapter,
    receipt: &Receipt,
    client: ClientId,
    page: &PageArgs,
    size: usize,
) -> Value {
    let mut value = results::receipt(receipt);
    let source = cursor::digest(&serde_json::to_vec(&value["result"]).expect("outcome JSON"));
    // Scope by operation, not selector: a page continues by its ID or original key.
    let query = format!("id:{}", wire::operation(receipt.id));
    let scope = Scope {
        client,
        epoch: receipt.metadata.epoch,
        query: &query,
        source: &source,
        size,
    };
    let offset = match adapter.cursors.decode(page.cursor.as_deref(), &scope) {
        Ok(v) => v,
        Err(code) => return results::failure(code, None),
    };
    let next_cursor = if let Some(candidates) = value["result"]
        .get_mut("candidates")
        .and_then(Value::as_array_mut)
    {
        if offset > candidates.len() {
            return results::failure("stale_cursor", None);
        }
        let total = candidates.len();
        *candidates = candidates.iter().skip(offset).take(size).cloned().collect();
        let next = offset + candidates.len();
        (next < total).then(|| adapter.cursors.encode(next, &scope))
    } else {
        if offset != 0 {
            return results::failure("stale_cursor", None);
        }
        None
    };
    for field in ["name", "model"] {
        if let Some(text) = value["result"].get(field).and_then(Value::as_str) {
            let (text, truncated) = results::preview(text);
            value["result"][field] = json!(text);
            value["result"][format!("{field}_truncated")] = json!(truncated);
        }
    }
    value["next_cursor"] = json!(next_cursor);
    value
}

async fn mutation(
    adapter: &Adapter,
    name: &str,
    mut arguments: Value,
    service: &ControlService,
    client: &AuthorizedClient,
    snapshot: &Snapshot,
) -> Value {
    let device = arguments["device_id"].as_str().unwrap_or("").to_owned();
    let operation = if name == "cancel_wake" {
        arguments
            .as_object_mut()
            .expect("validated arguments")
            .remove("operation_id")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let route = match name {
        "discover_tvs" => MutationRoute::Discover,
        "probe_tv" => MutationRoute::Probe,
        "pair_tv" => MutationRoute::Pair,
        "select_tv" => MutationRoute::Select,
        "connect_tv" => MutationRoute::Connect,
        "set_wake" => MutationRoute::SetWake(&device),
        "remote_action" => MutationRoute::Remote,
        "wake_tv" => MutationRoute::Wake,
        "cancel_wake" => MutationRoute::CancelWake(&operation),
        "forget_tv" => MutationRoute::Forget(&device),
        "recover_pairing" => MutationRoute::RecoverPairing(&device),
        "recover_forget" => MutationRoute::RecoverForget(&device),
        _ => return results::failure("invalid_request", Some(snapshot)),
    };
    let bytes = serde_json::to_vec(&arguments).expect("argument JSON");
    let request = match request::parse_mutation(route, &bytes, snapshot.epoch) {
        Ok(v) => v,
        Err(error) => {
            return results::failure(transport::request_error(error).code, Some(snapshot))
        }
    };
    match service
        .submit_authorized(request, client.admission.clone())
        .await
    {
        Ok(receipt) => {
            // Inspect only; a pending observation still returns its original
            // receipt immediately. This uses neither a subscriber nor a wait
            // reservation and never submits the command a second time.
            let receipt =
                if receipt.outcome.is_none() && matches!(name, "discover_tvs" | "probe_tv") {
                    match service.operation(snapshot.epoch, receipt.id).await {
                        Ok(Lookup::Known(current)) => current,
                        _ => receipt,
                    }
                } else {
                    receipt
                };
            let mut value = project_receipt(
                adapter,
                &receipt,
                client.identity,
                &PageArgs::default(),
                catalog::PAGE_DEFAULT,
            );
            if value.get("error").is_some() {
                let fresh = service.snapshot().await.ok();
                value["current"] = results::current(fresh.as_ref().unwrap_or(snapshot));
            }
            value
        }
        Err(error) => results::failure(response::service_error(error).code, Some(snapshot)),
    }
}

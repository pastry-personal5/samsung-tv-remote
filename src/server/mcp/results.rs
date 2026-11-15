//! Bounded model-visible projections; service facts keep their original meaning.
use super::catalog::{counter_schema, object, uuid_schema};
use crate::application::control_service::{Receipt, Snapshot};
use crate::server::control_http::{response, wire};
use serde_json::{json, Value};

pub const MAX_TEXT_CHARS: usize = 80;
pub const MAX_RESULT_BYTES: usize = 128 * 1024;

fn capability_schema() -> Value {
    let context = object(
        json!({"selection_generation":counter_schema(),"session":counter_schema()}),
        &["selection_generation", "session"],
    );
    let fact = object(
        json!({
            "key":{"enum":["installed_apps","app_launch","native_ime","hdmi1","hdmi2","hdmi3","tuner","sources"]},
            "support":{"enum":["unknown","supported","unsupported"]},
            "source":{"enum":["none","tv_response","tv_event","user_confirmation"]},
            "freshness":{"enum":["unobserved","current","stale"]},
            "observed_at_unix_ms":nullable(counter_schema()),"context":nullable(context)
        }),
        &[
            "key",
            "support",
            "source",
            "freshness",
            "observed_at_unix_ms",
            "context",
        ],
    );
    object(
        json!({"device_id":uuid_schema(),"evidence_epoch":nullable(uuid_schema()),"reason":{"enum":["evidence_missing","evidence_stale","not_selected","disconnected","recovery_blocked","feature_unsupported","connected","observed"]},
        "ime_active":{"type":"boolean"},"facts":{"type":"array","minItems":8,"maxItems":8,"items":fact}}),
        &[
            "device_id",
            "evidence_epoch",
            "reason",
            "ime_active",
            "facts",
        ],
    )
}

pub fn current(snapshot: &Snapshot) -> Value {
    json!({"server_epoch":wire::epoch(snapshot.epoch),"selected":snapshot.control.selected_device().map(wire::device),"selection_generation":wire::Counter(snapshot.control.selection_generation())})
}
pub fn preview(text: &str) -> (String, bool) {
    (
        text.chars().take(MAX_TEXT_CHARS).collect(),
        text.chars().count() > MAX_TEXT_CHARS,
    )
}
pub fn failure(code: &'static str, snapshot: Option<&Snapshot>) -> Value {
    let mut value = json!({"error":{"code":code,"next":guidance(code)}});
    if let Some(snapshot) = snapshot {
        value["current"] = current(snapshot);
    }
    value
}
pub fn guidance(code: &str) -> &'static str {
    match code {
        "stale_epoch" | "unknown_after_restart" => "Read list_tvs. The old effect is unknown; do not replay it automatically.",
        "outcome_unavailable" => "Outcome is unknown after eviction or non-admission; require a new explicit intent before any repeat.",
        "stale_cursor" | "invalid_cursor" => "Restart this read without a cursor.",
        "stale_selection" | "stale_config" => "Read the target's current guards before a new explicit intent.",
        "invalid_confirmation" => "Obtain explicit matching target or full certificate confirmation.",
        "busy" => "Capacity is occupied. Recover an existing intent by its original key; do not allocate a retry key.",
        "observation_expired" => "Probe again and obtain a fresh certificate confirmation before Pairing.",
        "partial_setup" => "Inspect the target's recovery transaction before guarded cleanup.",
        "unauthenticated" => "Reconnect using the configured auth helper; revoked access needs explicit Connect.",
        "not_found" => "Read list_tvs and inspect a saved target from that snapshot.",
        "storage_blocked" | "storage_corrupt" | "storage_unavailable" => "Resolve the blocked storage state in the GUI before a new setup intent.",
        _ => "Inspect the current state and correct the request; never replay an unknown effect automatically.",
    }
}

pub fn receipt(receipt: &Receipt) -> Value {
    let (_, mut value) = response::receipt(receipt);
    value["operation_key"] = json!(receipt.key.as_ref());
    if let Some(code) = value
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str)
    {
        let next = guidance(code);
        value["error"]["next"] = json!(next);
    }
    value
}
pub fn tool(value: Value) -> Value {
    let is_error = value.get("error").is_some();
    // Preserve the compatible text copy until actual client measurement proves
    // that omitting it is supported. Count both channels in budget evidence.
    let text = serde_json::to_string(&value).expect("JSON value serialization");
    json!({"structuredContent":value,"content":[{"type":"text","text":text}],"isError":is_error})
}

fn nullable(schema: Value) -> Value {
    json!({"anyOf":[schema,{"type":"null"}]})
}
fn error_schema() -> Value {
    object(
        json!({"code":{"type":"string"},"next":{"type":"string"}}),
        &["code", "next"],
    )
}
fn context_schema() -> Value {
    object(
        json!({"server_epoch":uuid_schema(),"selected":nullable(uuid_schema()),"selection_generation":counter_schema()}),
        &["server_epoch", "selected", "selection_generation"],
    )
}
fn wake_schema() -> Value {
    object(
        json!({"stage":{"type":"string"},"packet":{"enum":["not_sent","sent","uncertain"]},"remote":{"enum":["not_ready","ready","unknown"]},"failure":{"type":["string","null"]}}),
        &["stage", "packet", "remote", "failure"],
    )
}
fn outcome_schema() -> Value {
    // These fields mirror the exhaustive typed Outcome projection in /v1.
    object(
        json!({
            "kind":{"enum":["discovered","probed","tv_approved_and_saved","saved","remote_ready","written","uncertain","not_sent","wake","forgotten","partial_setup","failed"]},
            "candidates":{"type":"array","maxItems":super::catalog::PAGE_MAX,"items":object(json!({"candidate_id":uuid_schema(),"host":{"type":"string"},"expires_at_unix_ms":counter_schema()}), &["candidate_id","host","expires_at_unix_ms"])},
            "attempt_id":uuid_schema(),"observation_id":uuid_schema(),"host":{"type":"string"},"fingerprint":{"type":"string","pattern":"^[0-9a-f]{64}$"},
            "name":{"type":["string","null"]},"name_truncated":{"type":"boolean"},"model":{"type":["string","null"]},"model_truncated":{"type":"boolean"},
            "expires_at_unix_ms":counter_schema(),"device_id":uuid_schema(),"reason":{"type":"string"},"facts":wake_schema(),
            "credential_removed":{"type":"boolean"},"trust_removed":{"type":"boolean"},"record_removed":{"type":"boolean"},
            "error":object(json!({"code":{"type":"string"}}), &["code"])
        }),
        &["kind"],
    )
}

pub(super) fn schema(name: &str) -> Value {
    let mut properties = json!({"error":error_schema(),"current":context_schema()});
    let required: Vec<&str> = match name {
        "list_tvs" => {
            properties.as_object_mut().unwrap().extend(json!({
                "server_epoch":uuid_schema(),"revision":counter_schema(),"selection_generation":counter_schema(),"selected":nullable(uuid_schema()),
                "startup":{"enum":["restoring","ready","storage_unavailable","storage_corrupt","recovery_required","draining"]},
                "admission_closed":{"type":"boolean"},"recovery_required":{"type":"boolean"},"recovery_targets":{"type":"array","maxItems":2,"items":object(json!({"device_id":uuid_schema(),"kind":{"enum":["first_pair","re_pair","forget"]}}), &["device_id","kind"])},"storage_pending":{"type":"boolean"},"duplicate_records":{"type":"boolean"},
                "connection":{"enum":["not_connected","connecting","ready","failed"]},"pairing":{"enum":["not_started","in_progress","ready","failed"]},"wake":wake_schema(),
                "tvs":{"type":"array","maxItems":super::catalog::PAGE_MAX,"items":object(json!({"device_id":uuid_schema(),"label":{"type":"string","maxLength":MAX_TEXT_CHARS},"label_truncated":{"type":"boolean"},"config_revision":counter_schema()}), &["device_id","label","label_truncated","config_revision"])},
                "next_cursor":{"type":["string","null"]}
            }).as_object().unwrap().clone());
            vec![
                "server_epoch",
                "revision",
                "selection_generation",
                "selected",
                "startup",
                "admission_closed",
                "recovery_required",
                "recovery_targets",
                "storage_pending",
                "duplicate_records",
                "connection",
                "pairing",
                "wake",
                "tvs",
                "next_cursor",
            ]
        }
        "inspect_tv" => {
            properties.as_object_mut().unwrap().extend(json!({
                "server_epoch":uuid_schema(),"revision":counter_schema(),"selected":nullable(uuid_schema()),"selection_generation":counter_schema(),
                "startup":{"type":"string"},"admission_closed":{"type":"boolean"},
                "tv":nullable(object(json!({"capabilities":capability_schema(),"device_id":uuid_schema(),"label":{"type":"string","maxLength":MAX_TEXT_CHARS},"label_truncated":{"type":"boolean"},"host":{"type":"string"},"config_revision":counter_schema(),"wake":object(json!({"wired_mac":{"type":["string","null"]},"wifi_mac":{"type":["string","null"]},"active_interface":{"enum":["Wired","WiFi",null]}}), &["wired_mac","wifi_mac","active_interface"]),"sources":{"type":"object"}}), &["device_id","label","label_truncated","host","config_revision","wake","sources","capabilities"])),
                "connection":{"type":"string"},"pairing":{"type":"string"},"wake":nullable(wake_schema()),
                "forget_recovery":{"type":["object","null"]},"pairing_recovery":{"type":["object","null"]}
            }).as_object().unwrap().clone());
            vec![
                "server_epoch",
                "revision",
                "selected",
                "selection_generation",
                "startup",
                "admission_closed",
                "tv",
                "connection",
                "pairing",
                "wake",
                "forget_recovery",
                "pairing_recovery",
            ]
        }
        _ => {
            properties.as_object_mut().unwrap().extend(json!({
                "server_epoch":uuid_schema(),"operation_key":{"type":"string"},"operation_id":uuid_schema(),"device_id":nullable(uuid_schema()),"selection_generation":nullable(counter_schema()),
                "state":{"enum":["pending","terminal","unknown"]},"result":nullable(outcome_schema()),"expires_at_unix_ms":counter_schema(),
                "terminal_state":object(json!({"selected":nullable(uuid_schema()),"selection_generation":counter_schema(),"config_revision":nullable(counter_schema())}), &["selected","selection_generation","config_revision"]),
                "next_cursor":{"type":["string","null"]}
            }).as_object().unwrap().clone());
            vec![
                "server_epoch",
                "operation_key",
                "operation_id",
                "device_id",
                "selection_generation",
                "state",
                "result",
                "next_cursor",
            ]
        }
    };
    let mut schema = object(properties, &[]);
    schema["anyOf"] = json!([{"required":required},{"required":["error"]}]);
    schema
}

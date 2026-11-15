//! One stable semantic catalog. State changes never change these definitions.
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

pub const INSTRUCTIONS: &str = "Control Samsung TVs. Read list_tvs for epoch and selection. Allocate one UUID key per intent. Receipts are not physical confirmation. Recover lost replies by original key; never repeat unknown or uncertain effects automatically. Pair/Forget confirmations require user intent.";
pub const CACHE_TTL_MS: u32 = 300_000;
pub const PAGE_DEFAULT: usize = 4;
pub const PAGE_MAX: usize = 16;

const TOOLS: [(&str, &str, &[&str]); 15] = [
    ("cancel_wake", "Cancel a named Wake reconnect wait; a sent packet cannot be recalled.", &["server_epoch", "operation_key", "device_id", "selection_generation", "operation_id"]),
    ("connect_tv", "Connect the selected TV under the observed generation.", &["server_epoch", "operation_key", "device_id", "selection_generation"]),
    ("discover_tvs", "Discover local Samsung TVs; lookup returns expiring candidates.", &["server_epoch", "operation_key"]),
    ("forget_tv", "Forget a saved TV after explicit matching target confirmation.", &["server_epoch", "operation_key", "device_id", "expected_config_revision", "confirm_device_id"]),
    ("get_operation", "Recover a receipt/outcome by exactly one original key or ID. Waiting never repeats the effect.", &["server_epoch", "operation_id?", "operation_key?", "wait_seconds?", "page_size?", "cursor?"]),
    ("inspect_tv", "Inspect one saved TV's configuration and recovery guards; no physical state inference.", &["device_id"]),
    ("list_tvs", "List saved TVs, selected target, epoch, generation and startup/recovery state.", &["page_size?", "cursor?"]),
    ("pair_tv", "Pair/re-pair after user confirmation of one fresh observation and full fingerprint; TV approval remains required.", &["server_epoch", "operation_key", "host", "attempt_id", "observation_id", "fingerprint", "confirm_fingerprint", "label", "selection_generation", "device_id?", "recovery_transaction_id?"]),
    ("probe_tv", "Probe a local host and observe its certificate; optionally bind a discovered candidate.", &["server_epoch", "operation_key", "host", "candidate_id?"]),
    ("recover_forget", "Retry only an interrupted Forget transaction's remaining cleanup after target confirmation.", &["server_epoch", "operation_key", "device_id", "transaction_id", "confirm_device_id"]),
    ("recover_pairing", "Compensate an interrupted first Pair after exact target/transaction confirmation; re-pair uses pair_tv.", &["server_epoch", "operation_key", "device_id", "transaction_id", "confirm_device_id"]),
    ("remote_action", "Send one semantic remote action. Written means socket write; uncertain is never safe to replay.", &["server_epoch", "operation_key", "device_id", "selection_generation", "action"]),
    ("select_tv", "Change every client's selected TV using the generation observed; same-ID selection also advances it.", &["server_epoch", "operation_key", "device_id", "expected_generation"]),
    ("set_wake", "Set selected TV Wake MAC/interface configuration with its observed config revision.", &["server_epoch", "operation_key", "device_id", "selection_generation", "expected_config_revision", "wired_mac?", "wifi_mac?", "active_interface?"]),
    ("wake_tv", "Wake the selected TV once using captured configuration; inspect packet and readiness separately.", &["server_epoch", "operation_key", "device_id", "selection_generation"]),
];

pub fn is_read(name: &str) -> bool {
    matches!(name, "list_tvs" | "inspect_tv" | "get_operation")
}
pub fn contains(name: &str) -> bool {
    TOOLS.iter().any(|(n, _, _)| *n == name)
}

pub(super) fn uuid_schema() -> Value {
    json!({"type":"string","format":"uuid"})
}
pub(super) fn counter_schema() -> Value {
    json!({"type":"string","pattern":"^(0|[1-9][0-9]{0,19})$","maxLength":20})
}
pub(super) fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn input_field(name: &str) -> Value {
    match name {
        "page_size" => {
            json!({"type":"integer","minimum":1,"maximum":PAGE_MAX,"default":PAGE_DEFAULT})
        }
        "cursor" => json!({"type":"string","maxLength":1024}),
        "wait_seconds" => json!({"type":"integer","minimum":0,"maximum":10,"default":0}),
        "selection_generation" | "expected_generation" | "expected_config_revision" => {
            counter_schema()
        }
        "fingerprint" | "confirm_fingerprint" => {
            json!({"type":"string","pattern":"^[0-9a-f]{64}$"})
        }
        "label" => json!({"type":"string","minLength":1,"maxLength":80}),
        "host" => json!({"type":"string","minLength":1,"maxLength":253}),
        "wired_mac" | "wifi_mac" => {
            json!({"type":["string","null"],"pattern":"^([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}$"})
        }
        "active_interface" => json!({"enum":["Wired","WiFi",null]}),
        "action" => {
            json!({"enum":["PowerToggle","Up","Down","Left","Right","Enter","Back","Home","PlayPause","Play","Pause","Mute","VolumeUp","VolumeDown","Hdmi1","Hdmi2","Hdmi3","Tuner","SourceMenu"]})
        }
        _ => uuid_schema(),
    }
}

pub fn tools() -> &'static Value {
    static CATALOG: OnceLock<Value> = OnceLock::new();
    CATALOG.get_or_init(|| {
        Value::Array(
            TOOLS
                .iter()
                .map(|(name, description, fields)| {
                    let mut properties = Map::new();
                    let mut required = Vec::new();
                    for field in *fields {
                        let key = field.trim_end_matches('?');
                        properties.insert(key.into(), input_field(key));
                        if !field.ends_with('?') {
                            required.push(key);
                        }
                    }
                    let mut input = object(Value::Object(properties), &required);
                    if *name == "get_operation" {
                        input["oneOf"] =
                            json!([{"required":["operation_id"]},{"required":["operation_key"]}]);
                    }
                    let read = is_read(name);
                    json!({"name":name,"description":description,"inputSchema":input,
            "outputSchema":super::results::schema(name),
            "annotations":{"readOnlyHint":read,"idempotentHint":read,
                "destructiveHint":matches!(*name,"forget_tv"|"recover_forget"|"recover_pairing"),
                "openWorldHint":!read}})
                })
                .collect(),
        )
    })
}

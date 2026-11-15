//! Signed foreground operator controller. Attachment and report reads have no TV effects.
use crate::application::diagnostic::Experiment;
use crate::control_client::http::HttpClient;
use crate::server::{
    bootstrap::Role,
    launch::{LaunchError, Launcher},
    lifecycle::endpoint::build_identity,
    profile::Profile,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn command() -> Result<Option<String>, LaunchError> {
    let mut bytes = Vec::new();
    let mut stdin = std::io::stdin().lock();
    loop {
        let mut one = [0];
        match stdin.read(&mut one) {
            Ok(0) => return Ok(None),
            Ok(_) if one[0] == b'\n' => {
                return String::from_utf8(bytes)
                    .map(|s| Some(s.trim().into()))
                    .map_err(|_| LaunchError::Denied)
            }
            Ok(_) if bytes.len() < 128 => bytes.push(one[0]),
            _ => return Err(LaunchError::Denied),
        }
    }
}
fn experiment(value: &str) -> Option<Experiment> {
    Some(match value {
        "catalog" => Experiment::Catalog,
        "ime" => Experiment::Ime,
        "launch-a" => Experiment::LaunchA,
        "launch-b" => Experiment::LaunchB,
        "hdmi1" => Experiment::Hdmi1,
        "hdmi2" => Experiment::Hdmi2,
        "hdmi3" => Experiment::Hdmi3,
        "tuner" => Experiment::Tuner,
        "sources" => Experiment::Sources,
        _ => return None,
    })
}
async fn status(client: &HttpClient, lease: &str) -> Result<Value, LaunchError> {
    let (code, value) = client
        .call("GET", &format!("/_p3/status?lease={lease}"), None)
        .await
        .map_err(LaunchError::Client)?;
    if code != 200 {
        return Err(LaunchError::Unavailable);
    }
    Ok(value)
}
fn display(status: &Value) {
    // TV names are quoted data for operator choice; never included in exports.
    println!(
        "{}",
        json!({"choices":status["apps"],"aliases_set":status["aliases_set"],
        "closed":status["closed"],"catalog_quarantined":status["catalog_quarantined"],"launch_quarantined":status["launch_quarantined"],"records":status["records"]})
    );
}
fn with_shape(event: &str, shape: &str) -> Option<Value> {
    let mut value = json!({"event":event});
    let data = match shape {
        "absent" => return Some(value),
        "null" => Value::Null,
        "object" => json!({}),
        "array" => json!([]),
        "string" => json!("<redacted>"),
        "number" => json!(0),
        "boolean" => json!(false),
        _ => return None,
    };
    value["data"] = data;
    Some(value)
}
fn export(
    directory: &Path,
    index: usize,
    condition: &str,
    metadata: &Value,
    status: &Value,
) -> Result<PathBuf, LaunchError> {
    let typed: crate::application::diagnostic::DiagnosticStatus =
        serde_json::from_value(status.clone()).map_err(|_| LaunchError::UnsafeMetadata)?;
    if typed.records.len() > crate::application::diagnostic::MAX_INTENTS
        || typed.records.iter().any(|r| {
            r.catalog_unicode_names
                .as_ref()
                .is_some_and(|v| v.len() > crate::application::diagnostic::MAX_APPS)
        })
    {
        return Err(LaunchError::UnsafeMetadata);
    }
    let mut safe = serde_json::to_value(typed).map_err(|_| LaunchError::UnsafeMetadata)?;
    let object = safe.as_object_mut().ok_or(LaunchError::UnsafeMetadata)?;
    object.remove("apps");
    object.insert("lease".into(), json!("session-1"));
    let mut fixtures = Vec::new();
    let records = object
        .get_mut("records")
        .and_then(Value::as_array_mut)
        .ok_or(LaunchError::UnsafeMetadata)?;
    for (ordinal, record) in records.iter_mut().enumerate() {
        record["intent"] = json!(format!("intent-{}", ordinal + 1));
        record["sequence"] = json!((ordinal + 1).to_string());
        if record["catalog_events"] == 1 {
            if let Some(unicode) = record["catalog_unicode_names"].as_array() {
                let apps: Vec<_> = unicode.iter().enumerate().map(|(i, u)| json!({"appId":format!("synthetic-app-{:03}", i + 1),
                    "name":if u == &Value::Bool(true) {format!("합성 앱 {} 🎬", i + 1)} else {format!("App {}", i + 1)}})).collect();
                fixtures.push(json!({"experiment_sequence":record["sequence"],"frame":{"event":"ed.installedApp.get","data":{"data":apps}}}));
            }
        }
        for (field, event) in [
            ("launch_shape", "ed.apps.launch"),
            ("ime_start_shape", "ms.remote.imeStart"),
            ("ime_end_shape", "ms.remote.imeEnd"),
        ] {
            if let Some(frame) = record[field].as_str().and_then(|s| with_shape(event, s)) {
                fixtures.push(json!({"experiment_sequence":record["sequence"],"frame":frame}));
            }
        }
    }
    let digests: Vec<_> = fixtures
        .iter()
        .map(|fixture| {
            let bytes = serde_json::to_vec(&fixture["frame"]).expect("finite fixture");
            format!("{:x}", Sha256::digest(bytes))
        })
        .collect();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| LaunchError::Unavailable)?
        .as_millis()
        .to_string();
    let report = json!({"format":"p3-spike-v1","provenance":if metadata["synthetic"] == true {"synthetic"} else {"captured_sanitized_subset"},"review_required":true,"scope":"diagnostic_hypothesis_only","condition":condition,"target":"target-1","session":"session-1",
        "exported_at_unix_ms":now,"build":metadata,"redactions":["app IDs and names replaced","unknown fields removed","IME payload values removed","actual lease and intent identifiers replaced"],
        "machine":safe,"operator_observations":"record separately with the offline matrix recorder; machine observations do not prove panel state",
        "fixtures":fixtures,"fixture_sha256":digests});
    let path = directory.join(format!("report-{index:03}.json"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|_| LaunchError::Unavailable)?;
    file.write_all(&serde_json::to_vec_pretty(&report).map_err(|_| LaunchError::Unavailable)?)
        .map_err(|_| LaunchError::Unavailable)?;
    file.sync_all().map_err(|_| LaunchError::Unavailable)?;
    Ok(path)
}

pub async fn run(
    profile: Profile,
    executable: PathBuf,
    condition: String,
    directory: PathBuf,
) -> Result<String, LaunchError> {
    if !matches!(
        condition.as_str(),
        "baseline" | "reconnect" | "wake-reconnect"
    ) || !directory.is_absolute()
        || directory.exists()
    {
        return Err(LaunchError::Denied);
    }
    let production = matches!(profile, Profile::Control);
    let hash = build_identity(&executable).map_err(LaunchError::Endpoint)?;
    let client = Launcher::new(profile)?
        .attach(Role::Gui, Some(&hash))
        .await?;
    let source = executable
        .parent()
        .and_then(Path::parent)
        .map(|p| p.join("Resources/p3-spike-build.json"))
        .ok_or(LaunchError::UnsafeMetadata)?;
    let mut metadata = if production {
        let file = std::fs::File::open(source).map_err(|_| LaunchError::UnsafeMetadata)?;
        let mut bytes = Vec::new();
        file.take(4097)
            .read_to_end(&mut bytes)
            .map_err(|_| LaunchError::UnsafeMetadata)?;
        if bytes.len() > 4096 {
            return Err(LaunchError::UnsafeMetadata);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| LaunchError::UnsafeMetadata)?;
        let commit = value["commit"]
            .as_str()
            .ok_or(LaunchError::UnsafeMetadata)?;
        if !matches!(commit.len(), 40 | 64)
            || !commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !value["dirty"].is_boolean()
        {
            return Err(LaunchError::UnsafeMetadata);
        }
        json!({"commit":commit,"dirty":value["dirty"]})
    } else {
        json!({"synthetic":true})
    };
    metadata["executable_sha256"] = json!(hash);
    metadata["apple_signature_verified"] = json!(production);
    let (code, state) = client
        .call("GET", "/v1/state", None)
        .await
        .map_err(LaunchError::Client)?;
    if code != 200
        || state["connection"] != "ready"
        || state["pairing"] != "ready"
        || !state["selected"].is_string()
    {
        return Err(LaunchError::Denied);
    }
    let stream = client.diagnostic_lease(&json!({"server_epoch":state["server_epoch"],"device_id":state["selected"],"selection_generation":state["selection_generation"]})).await.map_err(LaunchError::Client)?;
    std::fs::create_dir(&directory).map_err(|_| LaunchError::Unavailable)?;
    let lease = stream.id.clone();
    let mut index = 0;
    println!("Diagnostic lease acquired for target-1. Commands: catalog; aliases A_INDEX B_INDEX; ime; confirm launch-a|launch-b|hdmi1|hdmi2|hdmi3|tuner|sources; status; export; quit. Every confirm is one explicit effect intent. IME observation sends no text. Wake/reconnect remain separate operator steps.");
    let mut timed_out = false;
    'session: while stream.is_alive() {
        let Some(input) = command()? else { break };
        if input == "quit" {
            break;
        }
        if input == "status" {
            display(&status(&client, &lease).await?);
            continue;
        }
        if input == "export" {
            let current = status(&client, &lease).await?;
            let path = export(&directory, index, &condition, &metadata, &current)?;
            index += 1;
            println!("Sanitized evidence draft: {}", path.display());
            continue;
        }
        if let Some(pair) = input.strip_prefix("aliases ") {
            let values: Vec<_> = pair.split_whitespace().collect();
            if values.len() != 2 {
                println!("Use aliases A_INDEX B_INDEX from this session's displayed choices.");
                continue;
            }
            let (Ok(a), Ok(b)) = (values[0].parse::<usize>(), values[1].parse::<usize>()) else {
                println!("Invalid alias indices.");
                continue;
            };
            let (code, current) = client
                .call(
                    "POST",
                    "/_p3/aliases",
                    Some(&json!({"lease":lease,"a":a,"b":b})),
                )
                .await
                .map_err(LaunchError::Client)?;
            if code == 200 {
                display(&current);
            } else {
                println!("Alias choice rejected; no TV request sent.");
            }
            continue;
        }
        let selected = if input == "catalog" || input == "ime" {
            experiment(&input)
        } else {
            input
                .strip_prefix("confirm ")
                .and_then(experiment)
                .filter(|e| !matches!(e, Experiment::Catalog | Experiment::Ime))
        };
        let Some(selected) = selected else {
            println!("Unknown command; no TV request sent.");
            continue;
        };
        if !stream.is_alive() {
            break;
        }
        let key = uuid::Uuid::new_v4().to_string();
        let (code, _) = client.call("POST", "/_p3/intent", Some(&json!({"lease":lease,"intent":key,"experiment":selected,"confirm_experiment":selected}))).await.map_err(LaunchError::Client)?;
        if code != 200 {
            println!("Experiment rejected; do not replay an unknown effect. Inspect status or explicitly reconnect if quarantined.");
            continue;
        }
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(selected.window_seconds() + 5);
        loop {
            let current = status(&client, &lease).await?;
            if current["closed"] == true
                || current["records"].as_array().is_some_and(|records| {
                    records
                        .iter()
                        .any(|r| r["intent"] == key && r["complete"] == true)
                })
            {
                display(&current);
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                // Still close the lease and keep the session's evidence draft.
                timed_out = true;
                break 'session;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    drop(stream); // The owner revokes the lease and aborts its session, never a retry.
    let mut current = status(&client, &lease).await?;
    for _ in 0..30 {
        if current["closed"] == true {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        current = status(&client, &lease).await?;
    }
    let path = export(&directory, index, &condition, &metadata, &current)?;
    if timed_out {
        println!(
            "Experiment wait expired; sanitized draft: {}",
            path.display()
        );
        return Err(LaunchError::Unavailable);
    }
    Ok(format!(
        "Diagnostic controller closed; sanitized draft: {}",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_removes_catalog_names_ids_and_actual_controller_identifiers() {
        let dir = tempfile::tempdir().unwrap();
        let mut record = crate::application::diagnostic::ExperimentRecord::new(
            1,
            "PRIVATE_INTENT".into(),
            Experiment::Catalog,
        );
        record.catalog_events = 1;
        record.catalog_unicode_names = Some(vec![true]);
        record.ime_start_shape = Some(crate::application::diagnostic::PayloadShape::Object);
        record.ime_end_shape = Some(crate::application::diagnostic::PayloadShape::String);
        let input = json!({"lease":"PRIVATE_LEASE", "closed":false,"catalog_quarantined":false,"launch_quarantined":false,"aliases_set":false,
            "apps":[{"choice":0,"name":"PRIVATE_APP_NAME"}],"records":[record]});
        let path = export(
            dir.path(),
            0,
            "baseline",
            &json!({"synthetic":true}),
            &input,
        )
        .unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(!text.contains("PRIVATE_"));
        assert!(text.contains("synthetic-app-001"));
        assert!(export(dir.path(), 0, "baseline", &json!({}), &input).is_err());
    }
}

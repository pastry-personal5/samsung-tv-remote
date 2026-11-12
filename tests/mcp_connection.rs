//! Isolated user configuration plus real protected offline enrollment. No
//! owner Claude files, Keychain items, TV runtime or installed app are touched.
use samsung_tv_remote::server::{
    mcp::{
        client_config,
        connection::{Configuration, Connection},
        management::{self, Action},
    },
    profile::Profile,
};
use serde_json::{json, Value};
use std::path::PathBuf;

fn connection(directory: &tempfile::TempDir) -> Connection {
    Connection {
        profile: Profile::development(&directory.path().canonicalize().unwrap()).unwrap(),
        executable: PathBuf::from(env!("CARGO_BIN_EXE_samsung-tv-remote")),
        configuration: directory.path().join(".claude.json"),
    }
}
async fn status(connection: &Connection) -> Value {
    management::run(
        connection.profile.clone(),
        connection.executable.clone(),
        Action::Status,
    )
    .await
    .unwrap()
}
fn key() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[tokio::test]
async fn offline_connect_is_idempotent_disconnect_and_regrant_keep_configuration_separate() {
    let directory = tempfile::tempdir().unwrap();
    let connection = connection(&directory);
    let unrelated = json!({"theme":"synthetic", "mcpServers":{"other":{"type":"http","url":"https://example.invalid"}}});
    std::fs::write(
        &connection.configuration,
        serde_json::to_vec(&unrelated).unwrap(),
    )
    .unwrap();
    let connected = connection.connect(0, &key(), None).await.unwrap();
    let entry = connected.entry.unwrap();
    assert_eq!(connected.configuration, Configuration::Saved);
    assert!(connected.enrollment.unwrap().enabled);
    let initial = status(&connection).await;
    assert_eq!(initial["generation"], "1");
    assert_eq!(initial["server_running"], false);
    assert!(client_config::inspect(&connection.configuration).unwrap() == Some(entry.value()));
    connection
        .connect(1, &key(), Some(entry.clone()))
        .await
        .unwrap();
    assert_eq!(status(&connection).await, initial);
    let disconnected = connection
        .disconnect(1, &key(), entry.clone())
        .await
        .unwrap();
    assert_eq!(disconnected.configuration, Configuration::Removed);
    assert!(!disconnected.enrollment.unwrap().enabled);
    let document: Value =
        serde_json::from_slice(&std::fs::read(&connection.configuration).unwrap()).unwrap();
    assert_eq!(document, unrelated);
    assert_eq!(status(&connection).await["enabled"], false);
    connection.connect(2, &key(), None).await.unwrap();
    let regranted = status(&connection).await;
    assert_eq!(regranted["generation"], "3");
    assert_eq!(regranted["enabled"], true);
    assert_eq!(regranted["url"], initial["url"]);
    assert_eq!(regranted["server_running"], false);
    assert!(connection
        .profile
        .directory()
        .unwrap()
        .read("namespace", 64)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn configuration_conflict_prevents_fresh_grant_and_failed_removal_keeps_access_disabled() {
    let directory = tempfile::tempdir().unwrap();
    let connection = connection(&directory);
    let conflicting =
        json!({"mcpServers":{"samsung-tv-remote":{"type":"http","url":"https://example.invalid"}}});
    std::fs::write(
        &connection.configuration,
        serde_json::to_vec(&conflicting).unwrap(),
    )
    .unwrap();
    let failure = connection.connect(0, &key(), None).await.unwrap_err();
    assert!(!failure.progress.enrollment.unwrap().enabled);
    assert_eq!(status(&connection).await["generation"], "0");
    std::fs::write(&connection.configuration, b"{}").unwrap();
    let entry = connection
        .connect(0, &key(), None)
        .await
        .unwrap()
        .entry
        .unwrap();
    std::fs::write(
        &connection.configuration,
        serde_json::to_vec(&conflicting).unwrap(),
    )
    .unwrap();
    let failure = connection.disconnect(1, &key(), entry).await.unwrap_err();
    assert!(!failure.progress.enrollment.unwrap().enabled);
    assert_eq!(status(&connection).await["enabled"], false);
    let current: Value =
        serde_json::from_slice(&std::fs::read(&connection.configuration).unwrap()).unwrap();
    assert_eq!(current, conflicting);
}

#[tokio::test]
async fn lost_connect_reply_can_recover_with_original_generation_key_and_expected_entry() {
    let directory = tempfile::tempdir().unwrap();
    let connection = connection(&directory);
    let operation_key = key();
    connection.connect(0, &operation_key, None).await.unwrap();
    let initial = status(&connection).await;
    let bytes = std::fs::read(&connection.configuration).unwrap();
    let recovered = connection.connect(0, &operation_key, None).await.unwrap();
    assert_eq!(recovered.configuration, Configuration::Saved);
    assert_eq!(status(&connection).await, initial);
    assert_eq!(std::fs::read(&connection.configuration).unwrap(), bytes);
    connection
        .disconnect(1, &key(), recovered.entry.unwrap())
        .await
        .unwrap();
    assert!(connection.connect(0, &operation_key, None).await.is_err());
    assert!(client_config::inspect(&connection.configuration)
        .unwrap()
        .is_none());
}

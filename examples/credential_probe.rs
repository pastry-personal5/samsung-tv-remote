//! Disposable fresh-credential feasibility probe. The service is fixed outside
//! both production namespaces; values are synthetic and never printed.
use samsung_tv_remote::infrastructure::macos::server_keychain::ServerKeychain;
use std::io;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 || !matches!(args[0].as_str(), "create" | "read" | "update" | "delete") {
        return Err(io::Error::other(
            "usage: credential_probe create|read|update|delete <disposable-v4-uuid>",
        )
        .into());
    }
    let id = uuid::Uuid::parse_str(&args[1])?;
    if id.to_string() != args[1] || id.get_version_num() != 4 {
        return Err(io::Error::other("canonical disposable v4 UUID required").into());
    }
    let account = format!("probe-{}", args[1]);
    let store = ServerKeychain::disposable_probe()
        .map_err(|e| io::Error::other(format!("store: {e:?}")))?;
    let result = (|| {
        match args[0].as_str() {
            "create" => {
                if store.load(&account)?.is_some() {
                    return Err("account already exists".into());
                }
                store
                    .create(&account, b"m9-synthetic-first")
                    .map_err(|e| format!("create: {e:?}"))?;
                // A racing first-use provisioner must not replace this item.
                if store.create(&account, b"must-not-replace").is_ok() {
                    return Err("duplicate create replaced item".into());
                }
                if store.load(&account)? != Some(b"m9-synthetic-first".to_vec()) {
                    return Err("first value mismatch".into());
                }
            }
            "read" => {
                let value = store.load(&account)?;
                if !matches!(
                    value.as_deref(),
                    Some(b"m9-synthetic-first" | b"m9-synthetic-updated")
                ) {
                    return Err("synthetic value missing or changed".into());
                }
            }
            "update" => {
                if store.load(&account)? != Some(b"m9-synthetic-first".to_vec()) {
                    return Err("update requires first value".into());
                }
                store.save(&account, b"m9-synthetic-updated")?;
                if store.load(&account)? != Some(b"m9-synthetic-updated".to_vec()) {
                    return Err("update mismatch".into());
                }
            }
            "delete" => {
                store.delete(&account)?;
                if store.load(&account)?.is_some() {
                    return Err("delete did not remove item".into());
                }
            }
            _ => unreachable!(),
        }
        Ok::<(), ProbeError>(())
    })();
    match result {
        Ok(()) => {
            println!("{}: passed (synthetic value redacted)", args[0]);
            Ok(())
        }
        Err(error) => Err(io::Error::other(format!("{}: {}", args[0], error.0)).into()),
    }
}

struct ProbeError(String);
impl From<&str> for ProbeError {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}
impl From<String> for ProbeError {
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl From<samsung_tv_remote::infrastructure::macos::server_keychain::CredentialError>
    for ProbeError
{
    fn from(
        value: samsung_tv_remote::infrastructure::macos::server_keychain::CredentialError,
    ) -> Self {
        Self(format!("{value:?}"))
    }
}

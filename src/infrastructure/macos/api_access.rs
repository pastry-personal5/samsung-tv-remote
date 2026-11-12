//! Explicit foreground maintenance, dispatched before GUI/runtime creation.
//! Inspect performs no Keychain calls. Request reads only existing API roles.
//! Routine startup and credential recheck never invoke this module.
use crate::infrastructure::macos::{server_keychain::API_SERVICE, signed_peer::SignedPeer};
use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    boolean::CFBoolean,
    data::CFData,
    dictionary::CFDictionary,
    string::{CFString, CFStringRef},
};
use security_framework::os::macos::keychain::SecKeychain;
use security_framework_sys::{
    base::SecKeychainRef, item::*, keychain::SecKeychainSetUserInteractionAllowed,
    keychain_item::SecItemCopyMatching,
};
use std::{ffi::OsStr, io, path::Path, process::Command};

#[link(name = "Security", kind = "framework")]
extern "C" {
    static kSecUseAuthenticationUIAllow: CFStringRef;
    fn SecKeychainGetStatus(keychain: SecKeychainRef, status: *mut u32) -> i32;
}

fn codesign(args: &[&OsStr]) -> io::Result<String> {
    let output = Command::new("/usr/bin/codesign").args(args).output()?;
    if !output.status.success() {
        return Err(io::Error::other("signature validation failed"));
    }
    // Only public signing metadata is requested; never pass a credential here.
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

#[derive(Debug, PartialEq, Eq)]
struct Signature {
    requirement: String,
    team: String,
}
fn parse_signature(text: &str) -> io::Result<Signature> {
    let field = |prefix| text.lines().find_map(|line| line.strip_prefix(prefix));
    if field("Identifier=") != Some("dev.samsungtvremote.local") {
        return Err(io::Error::other("unexpected signing identifier"));
    }
    let team = field("TeamIdentifier=")
        .filter(|team| team.len() == 10 && team.bytes().all(|c| c.is_ascii_alphanumeric()))
        .ok_or_else(|| io::Error::other("Apple signing team required"))?;
    let requirement = field("designated => ")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::other("designated requirement missing"))?;
    Ok(Signature {
        requirement: requirement.to_owned(),
        team: team.to_owned(),
    })
}
fn signature(path: &Path) -> io::Result<Signature> {
    // Validate the actual Apple certificate chain, not just a metadata label.
    codesign(&[
        OsStr::new("--verify"),
        OsStr::new("--strict"),
        OsStr::new("-R=anchor apple generic"),
        path.as_os_str(),
    ])?;
    let text = codesign(&[
        OsStr::new("--display"),
        OsStr::new("--verbose=4"),
        OsStr::new("-r-"),
        path.as_os_str(),
    ])?;
    parse_signature(&text)
}

fn status(code: i32) -> &'static str {
    match code {
        -25300 => "missing",
        -25293 | -128 => "denied",
        -25308 | -25315 => "interaction_required",
        _ => "unavailable",
    }
}
fn read_role(keychain: &SecKeychain, account: &str) -> &'static str {
    // SAFETY: fixed Security constants and retained query values live through
    // the synchronous Copy. Its retained result is immediately owned by RAII.
    unsafe {
        let query = CFDictionary::from_CFType_pairs(&[
            (
                CFString::wrap_under_get_rule(kSecClass),
                CFString::wrap_under_get_rule(kSecClassGenericPassword).as_CFType(),
            ),
            (
                CFString::wrap_under_get_rule(kSecAttrService),
                CFString::new(API_SERVICE).as_CFType(),
            ),
            (
                CFString::wrap_under_get_rule(kSecAttrAccount),
                CFString::new(account).as_CFType(),
            ),
            (
                CFString::wrap_under_get_rule(kSecMatchSearchList),
                CFArray::from_CFTypes(std::slice::from_ref(keychain)).as_CFType(),
            ),
            (
                CFString::wrap_under_get_rule(kSecUseAuthenticationUI),
                CFString::wrap_under_get_rule(kSecUseAuthenticationUIAllow).as_CFType(),
            ),
            (
                CFString::wrap_under_get_rule(kSecReturnData),
                CFBoolean::true_value().as_CFType(),
            ),
        ]);
        let mut result = std::ptr::null();
        let code = SecItemCopyMatching(query.as_concrete_TypeRef(), &mut result);
        if code != 0 {
            return status(code);
        }
        let value = CFType::wrap_under_create_rule(result);
        match value.downcast::<CFData>() {
            Some(data) if data.len() == 32 => "available",
            _ => "malformed",
        }
    }
}
fn report(mut read: impl FnMut(&str) -> &'static str) -> [&'static str; 2] {
    let gui = read("gui");
    // Cancellation or a blocked/missing first item must not prompt again.
    let mcp = if gui == "available" {
        read("mcp")
    } else {
        "not_requested"
    };
    [gui, mcp]
}
fn request_access() -> [&'static str; 2] {
    let Ok(keychain) = SecKeychain::default() else {
        return ["unavailable", "not_requested"];
    };
    let mut state = 0;
    // SAFETY: a live default keychain and correctly typed output pointer.
    let code = unsafe { SecKeychainGetStatus(keychain.as_concrete_TypeRef(), &mut state) };
    if code != 0 {
        return [status(code), "not_requested"];
    }
    if state & 1 == 0 {
        return ["locked", "not_requested"];
    }
    struct DisableInteraction;
    impl Drop for DisableInteraction {
        fn drop(&mut self) {
            // SAFETY: no pointer inputs; this single-threaded tool is exiting.
            unsafe {
                SecKeychainSetUserInteractionAllowed(0);
            }
        }
    }
    let _guard = DisableInteraction;
    // SAFETY: explicit foreground maintenance, no runtime or worker threads.
    let code = unsafe { SecKeychainSetUserInteractionAllowed(1) };
    if code != 0 {
        return [status(code), "not_requested"];
    }
    report(|account| read_role(&keychain, account))
}

/// Validate the calling executable and optional standalone-helper candidate.
/// The application passes its own executable, so native approval applies to
/// the actual server binary rather than a different helper path.
pub fn run(inspect: bool, candidate_path: &Path) -> io::Result<serde_json::Value> {
    let own = SignedPeer::for_current_executable()
        .map_err(|_| io::Error::other("certificate-backed signature required"))?;
    let candidate = signature(candidate_path)?;
    let helper = signature(&std::env::current_exe()?)?;
    if candidate != helper || candidate.requirement != own.requirement() {
        return Err(io::Error::other(
            "helper and candidate signing identities differ",
        ));
    }
    let roles = if inspect {
        ["not_requested"; 2]
    } else {
        request_access()
    };
    Ok(serde_json::json!({
        "mode": if inspect { "inspect" } else { "request_access" },
        "signature_verified": true, "team": candidate.team,
        "service": API_SERVICE, "gui": roles[0], "mcp": roles[1],
        "credential_values_changed": false,
        "keychain_access_requested": !inspect,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_is_fixed_and_first_failure_stops_requests() {
        let mut calls = Vec::new();
        assert_eq!(
            report(|role| {
                calls.push(role.to_owned());
                "denied"
            }),
            ["denied", "not_requested"]
        );
        assert_eq!(calls, ["gui"]);
        calls.clear();
        assert_eq!(
            report(|role| {
                calls.push(role.to_owned());
                "available"
            }),
            ["available"; 2]
        );
        assert_eq!(calls, ["gui", "mcp"]);
        for code in [-25293, -128, -25308, -25315, -25291] {
            assert_ne!(status(code), "missing");
        }
    }
    #[test]
    fn metadata_requires_fixed_identifier_team_and_requirement() {
        let valid = "Identifier=dev.samsungtvremote.local\nTeamIdentifier=ABCDEFGHIJ\ndesignated => identifier test and anchor apple generic\n";
        assert!(parse_signature(valid).is_ok());
        for invalid in [
            valid.replace("ABCDEFGHIJ", "not set"),
            valid.replace("dev.samsungtvremote.local", "other"),
            valid.replace("designated => ", "absent => "),
        ] {
            assert!(parse_signature(&invalid).is_err());
        }
    }
}

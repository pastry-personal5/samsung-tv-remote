//! Authenticate a connected Unix peer by its kernel audit token and our own
//! certificate-backed designated requirement. Neither PID lookup nor a path
//! supplied by the peer participates in the decision.
use crate::server::bootstrap::{BootstrapError, PeerVerifier, Role};
use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    data::CFData,
    dictionary::{CFDictionary, CFDictionaryRef},
    number::CFNumber,
    string::{CFString, CFStringRef},
};
use security_framework_sys::code_signing::*;
use std::{os::fd::AsRawFd, ptr};
use tokio::net::UnixStream;

// These public Security APIs are not exposed by security-framework-sys yet.
#[link(name = "Security", kind = "framework")]
extern "C" {
    fn SecCodeCopyDesignatedRequirement(
        code: SecStaticCodeRef,
        flags: SecCSFlags,
        requirement: *mut SecRequirementRef,
    ) -> i32;
    fn SecRequirementCopyString(
        requirement: SecRequirementRef,
        flags: SecCSFlags,
        text: *mut CFStringRef,
    ) -> i32;
    fn SecCodeCopySigningInformation(
        code: SecStaticCodeRef,
        flags: SecCSFlags,
        information: *mut CFDictionaryRef,
    ) -> i32;
    static kSecCodeInfoFlags: CFStringRef;
}

fn checked(status: i32) -> Result<(), BootstrapError> {
    if status == 0 {
        Ok(())
    } else {
        Err(BootstrapError::Denied)
    }
}

/// A stable certificate/identifier requirement, obtained from this executable,
/// never from socket metadata, an environment variable or the remote process.
/// Signing identifies the executable, not its mode or human authorization.
pub struct SignedPeer {
    requirement: String,
    uid: u32,
}

impl SignedPeer {
    pub fn for_current_executable() -> Result<Self, BootstrapError> {
        // SAFETY: Security Copy functions return retained CF objects on success;
        // each is immediately wrapped using the create rule and released by RAII.
        unsafe {
            let mut code = ptr::null_mut();
            checked(SecCodeCopySelf(0, &mut code))?;
            let owner = CFType::wrap_under_create_rule(code.cast());
            checked(SecCodeCheckValidity(
                code,
                kSecCSStrictValidate,
                ptr::null_mut(),
            ))?;
            let mut information = ptr::null();
            checked(SecCodeCopySigningInformation(
                code.cast(),
                0,
                &mut information,
            ))?;
            let information = CFDictionary::<CFString, CFType>::wrap_under_create_rule(information);
            let flags = information
                .find(CFString::wrap_under_get_rule(kSecCodeInfoFlags))
                .and_then(|value| value.downcast::<CFNumber>())
                .and_then(|value| value.to_i64())
                .ok_or(BootstrapError::Denied)?;
            // Ad-hoc identities bind to a build hash, not a stable signer.
            if flags & 2 != 0 {
                return Err(BootstrapError::Denied);
            }
            let mut requirement = ptr::null_mut();
            checked(SecCodeCopyDesignatedRequirement(
                code.cast(),
                0,
                &mut requirement,
            ))?;
            let requirement_owner = CFType::wrap_under_create_rule(requirement.cast());
            let mut text = ptr::null();
            checked(SecRequirementCopyString(requirement, 0, &mut text))?;
            let text = CFString::wrap_under_create_rule(text).to_string();
            drop(requirement_owner);
            drop(owner);
            Ok(Self {
                requirement: text,
                uid: libc::geteuid(),
            })
        }
    }

    /// Safe to include in a sanitized platform report; contains no credential.
    pub fn requirement(&self) -> &str {
        &self.requirement
    }
}

impl PeerVerifier for SignedPeer {
    fn verify(&self, stream: &UnixStream, _role: Role) -> Result<(), BootstrapError> {
        if stream
            .peer_cred()
            .map_err(|_| BootstrapError::Denied)?
            .uid()
            != self.uid
        {
            return Err(BootstrapError::Denied);
        }
        // LOCAL_PEERTOKEN is 0x006 in the macOS SDK. The complete 32-byte audit
        // token includes pidversion; do not substitute a racy PID-based query.
        let mut token = [0_u8; 32];
        let mut length = token.len() as libc::socklen_t;
        // SAFETY: the kernel writes at most length bytes into this live buffer.
        let status = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                0,
                0x006,
                token.as_mut_ptr().cast(),
                &mut length,
            )
        };
        if status != 0 || length as usize != token.len() {
            return Err(BootstrapError::Denied);
        }
        let data = CFData::from_buffer(&token);
        // SAFETY: constants are borrowed CFStrings; dictionaries retain their
        // inputs. All CF values and the connected descriptor remain live.
        unsafe {
            let attributes = CFDictionary::from_CFType_pairs(&[(
                CFString::wrap_under_get_rule(kSecGuestAttributeAudit),
                data.as_CFType(),
            )]);
            let mut guest = ptr::null_mut();
            checked(SecCodeCopyGuestWithAttributes(
                ptr::null_mut(),
                attributes.as_concrete_TypeRef(),
                0,
                &mut guest,
            ))?;
            let guest_owner = CFType::wrap_under_create_rule(guest as CFTypeRef);
            let text = CFString::new(&self.requirement);
            let mut requirement = ptr::null_mut();
            checked(SecRequirementCreateWithString(
                text.as_concrete_TypeRef(),
                0,
                &mut requirement,
            ))?;
            let requirement_owner = CFType::wrap_under_create_rule(requirement.cast());
            let result = checked(SecCodeCheckValidity(
                guest,
                kSecCSStrictValidate,
                requirement,
            ));
            drop(requirement_owner);
            drop(guest_owner);
            result
        }
    }
}

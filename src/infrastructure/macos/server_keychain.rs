//! Fresh server-owned credentials. This adapter is deliberately separate from
//! the legacy pairing store and is not installed in the shipping composition
//! until the signed cutover gates pass.
use core_foundation::{
    base::{CFType, TCFType},
    boolean::CFBoolean,
    data::CFData,
    dictionary::CFDictionary,
    string::{CFString, CFStringRef},
};
use security_framework::os::macos::keychain::SecKeychain;
use security_framework_sys::{
    base::SecKeychainRef,
    item::*,
    keychain::SecKeychainSetUserInteractionAllowed,
    keychain_item::{SecItemAdd, SecItemCopyMatching, SecItemDelete, SecItemUpdate},
};

pub const PAIRING_SERVICE: &str = "dev.samsungtvremote.local.control-v1.pairing";
pub const API_SERVICE: &str = "dev.samsungtvremote.local.control-v1.api";
const PROBE_SERVICE: &str = "dev.samsungtvremote.local.m9-disposable";

#[link(name = "Security", kind = "framework")]
extern "C" {
    static kSecUseAuthenticationUIFail: CFStringRef;
    fn SecKeychainGetStatus(keychain: SecKeychainRef, status: *mut u32) -> i32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialError {
    Locked,
    Denied,
    InteractionRequired,
    Unavailable,
}
fn error(status: i32) -> CredentialError {
    match status {
        -25293 | -128 => CredentialError::Denied,
        -25308 | -25315 => CredentialError::InteractionRequired,
        _ => CredentialError::Unavailable,
    }
}

/// Construction disables legacy Keychain UI process-wide. Construct only in
/// the credential-owning server or a disposable probe, never in GUI mode.
pub struct ServerKeychain {
    service: &'static str,
}

/// TV credentials cannot accidentally use the API or disposable service.
pub struct ServerPairingCredentials(ServerKeychain);
impl ServerPairingCredentials {
    pub fn new() -> Result<Self, CredentialError> {
        Ok(Self(ServerKeychain::pairing()?))
    }
}
impl crate::application::credential_store::CredentialStore for ServerPairingCredentials {
    fn load(
        &self,
        device: crate::domain::DeviceId,
    ) -> Result<
        Option<crate::application::credential_store::PairingToken>,
        crate::application::credential_store::SecretError,
    > {
        use crate::application::credential_store::{PairingToken, SecretError};
        ServerKeychain::load(&self.0, &device.to_string())
            .map_err(|_| SecretError::Unavailable)?
            .map(|bytes| {
                String::from_utf8(bytes)
                    .ok()
                    .and_then(PairingToken::from_stored)
                    .ok_or(SecretError::InvalidToken)
            })
            .transpose()
    }
    fn save(
        &self,
        device: crate::domain::DeviceId,
        token: &crate::application::credential_store::PairingToken,
    ) -> Result<(), crate::application::credential_store::SecretError> {
        ServerKeychain::save(&self.0, &device.to_string(), token.as_str().as_bytes())
            .map_err(|_| crate::application::credential_store::SecretError::Unavailable)
    }
    fn delete(
        &self,
        device: crate::domain::DeviceId,
    ) -> Result<(), crate::application::credential_store::SecretError> {
        ServerKeychain::delete(&self.0, &device.to_string())
            .map_err(|_| crate::application::credential_store::SecretError::Unavailable)
    }
}
impl ServerKeychain {
    fn new(service: &'static str) -> Result<Self, CredentialError> {
        // SAFETY: this process-wide Security setting has no pointer inputs.
        let status = unsafe { SecKeychainSetUserInteractionAllowed(0) };
        if status != 0 {
            return Err(error(status));
        }
        Ok(Self { service })
    }
    pub fn pairing() -> Result<Self, CredentialError> {
        Self::new(PAIRING_SERVICE)
    }
    pub fn api() -> Result<Self, CredentialError> {
        Self::new(API_SERVICE)
    }
    /// Fixed separate service: a fixture can never choose the production name.
    pub fn disposable_probe() -> Result<Self, CredentialError> {
        Self::new(PROBE_SERVICE)
    }

    fn options(&self, account: &str) -> Result<Vec<(CFString, CFType)>, CredentialError> {
        // Detect a locked default store without attempting to unlock it. Other
        // authentication failures remain typed errors rather than item absence.
        let keychain = SecKeychain::default().map_err(|e| error(e.code()))?;
        let mut status = 0;
        // SAFETY: the live keychain and status output have the API's types.
        let result = unsafe { SecKeychainGetStatus(keychain.as_concrete_TypeRef(), &mut status) };
        if result != 0 {
            return Err(error(result));
        }
        if status & 1 == 0 {
            return Err(CredentialError::Locked);
        }
        // SAFETY: Security constants are immortal CFStrings. Every value is
        // retained by its wrapper and then by the query dictionary.
        unsafe {
            Ok(vec![
                (
                    CFString::wrap_under_get_rule(kSecClass),
                    CFString::wrap_under_get_rule(kSecClassGenericPassword).as_CFType(),
                ),
                (
                    CFString::wrap_under_get_rule(kSecAttrService),
                    CFString::new(self.service).as_CFType(),
                ),
                (
                    CFString::wrap_under_get_rule(kSecAttrAccount),
                    CFString::new(account).as_CFType(),
                ),
                (
                    CFString::wrap_under_get_rule(kSecUseAuthenticationUI),
                    CFString::wrap_under_get_rule(kSecUseAuthenticationUIFail).as_CFType(),
                ),
            ])
        }
    }
    pub fn load(&self, account: &str) -> Result<Option<Vec<u8>>, CredentialError> {
        let mut options = self.options(account)?;
        // SAFETY: all query inputs are retained through the call. A successful
        // Copy returns a retained CF object, owned immediately by RAII.
        unsafe {
            options.push((
                CFString::wrap_under_get_rule(kSecReturnData),
                CFBoolean::true_value().as_CFType(),
            ));
            let query = CFDictionary::from_CFType_pairs(&options);
            let mut result = std::ptr::null();
            let status = SecItemCopyMatching(query.as_concrete_TypeRef(), &mut result);
            match status {
                0 => {
                    let value = CFType::wrap_under_create_rule(result);
                    value
                        .downcast::<CFData>()
                        .map(|data| Some(data.to_vec()))
                        .ok_or(CredentialError::Unavailable)
                }
                -25300 => Ok(None),
                _ => Err(error(status)),
            }
        }
    }
    pub fn save(&self, account: &str, value: &[u8]) -> Result<(), CredentialError> {
        let query = CFDictionary::from_CFType_pairs(&self.options(account)?);
        // SAFETY: the query and data dictionary remain live through the call.
        let status = unsafe {
            let data = CFDictionary::from_CFType_pairs(&[(
                CFString::wrap_under_get_rule(kSecValueData),
                CFData::from_buffer(value),
            )]);
            SecItemUpdate(query.as_concrete_TypeRef(), data.as_concrete_TypeRef())
        };
        match status {
            0 => Ok(()),
            -25300 => self.create(account, value),
            _ => Err(error(status)),
        }
    }
    /// First-use creation never overwrites an item raced in by another owner.
    /// Call only after a successful load returned None; denial is never absence.
    pub fn create(&self, account: &str, value: &[u8]) -> Result<(), CredentialError> {
        let mut options = self.options(account)?;
        // SAFETY: options retains the CFData and dictionary for the whole call.
        let status = unsafe {
            options.push((
                CFString::wrap_under_get_rule(kSecValueData),
                CFData::from_buffer(value).as_CFType(),
            ));
            let query = CFDictionary::from_CFType_pairs(&options);
            SecItemAdd(query.as_concrete_TypeRef(), std::ptr::null_mut())
        };
        if status == 0 {
            Ok(())
        } else {
            Err(error(status))
        }
    }
    pub fn delete(&self, account: &str) -> Result<(), CredentialError> {
        let query = CFDictionary::from_CFType_pairs(&self.options(account)?);
        // SAFETY: query remains live through this synchronous call.
        match unsafe { SecItemDelete(query.as_concrete_TypeRef()) } {
            0 | -25300 => Ok(()),
            status => Err(error(status)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn errors_do_not_turn_authentication_failure_into_absence() {
        assert_eq!(error(-25293), CredentialError::Denied);
        assert_eq!(error(-128), CredentialError::Denied);
        assert_eq!(error(-25308), CredentialError::InteractionRequired);
        assert_eq!(error(-25315), CredentialError::InteractionRequired);
        assert_eq!(error(-25291), CredentialError::Unavailable);
        assert_ne!(PAIRING_SERVICE, "dev.samsungtvremote.local.pairing");
        assert_ne!(PAIRING_SERVICE, API_SERVICE);
        assert_ne!(PROBE_SERVICE, API_SERVICE);
        assert_ne!(PROBE_SERVICE, PAIRING_SERVICE);
    }
}

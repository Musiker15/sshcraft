//! FIDO2 tokens and Windows Hello through the Windows WebAuthn API (`webauthn.dll`).
//!
//! Since Windows 10 1903 an app may not talk to a token directly; Windows shows its own dialog
//! (touch, PIN, Hello) and hands back the result. The client data is the SSH message itself:
//! Windows hashes it with SHA-256, which is exactly the hash an `sk-` signature covers.

// The Windows API is only reachable through FFI. This module is one of the two places in the
// workspace that may use unsafe (AGENTS.md, "Never crash"); every block states why it is sound.
#![allow(unsafe_code)]

use windows::Win32::Foundation::HWND;
use windows::Win32::Networking::WindowsWebServices::{
    WEBAUTHN_API_VERSION_1, WEBAUTHN_API_VERSION_4, WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE,
    WEBAUTHN_AUTHENTICATOR_ATTACHMENT_CROSS_PLATFORM, WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM, WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS,
    WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS, WEBAUTHN_CLIENT_DATA, WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
    WEBAUTHN_COSE_ALGORITHM_ECDSA_P256_WITH_SHA256, WEBAUTHN_COSE_CREDENTIAL_PARAMETER, WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION,
    WEBAUTHN_COSE_CREDENTIAL_PARAMETERS, WEBAUTHN_CREDENTIAL, WEBAUTHN_CREDENTIAL_CURRENT_VERSION, WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
    WEBAUTHN_CREDENTIALS, WEBAUTHN_HASH_ALGORITHM_SHA_256, WEBAUTHN_RP_ENTITY_INFORMATION, WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
    WEBAUTHN_USER_ENTITY_INFORMATION, WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION, WEBAUTHN_USER_VERIFICATION_REQUIREMENT_DISCOURAGED,
    WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED, WebAuthNAuthenticatorGetAssertion, WebAuthNAuthenticatorMakeCredential,
    WebAuthNDeletePlatformCredential, WebAuthNFreeAssertion, WebAuthNFreeCredentialAttestation, WebAuthNGetApiVersionNumber, WebAuthNGetErrorName,
    WebAuthNIsUserVerifyingPlatformAuthenticatorAvailable,
};
use windows::core::{HRESULT, HSTRING, PCWSTR};

use crate::{Assertion, Attachment, Attestation, Authenticator, CreateOptions, FidoError, SkAlgorithm};

/// How long Windows waits for a touch or a PIN.
const TIMEOUT_MS: u32 = 120_000;
/// The option structures are filled only with fields of their first version, which every
/// Windows with WebAuthn understands.
const OPTIONS_VERSION: u32 = 1;
/// EdDSA (RFC 9053); the Windows headers this crate is generated from do not name it.
const COSE_ALGORITHM_EDDSA: i32 = -8;
const NTE_USER_CANCELLED: i32 = 0x8009_0036_u32 as i32;
const HRESULT_ERROR_CANCELLED: i32 = 0x8007_04C7_u32 as i32;

/// Asks Windows, with its dialogs owned by the window `hwnd`.
pub struct WebAuthn {
    /// Raw window handle, kept as an integer so the struct stays `Send`.
    pub hwnd: isize,
}

/// Whether this Windows has the WebAuthn API at all.
pub fn webauthn_available() -> bool {
    // SAFETY: a plain query without arguments.
    unsafe { WebAuthNGetApiVersionNumber() >= WEBAUTHN_API_VERSION_1 }
}

/// Whether Windows Hello can hold a key on this device.
pub fn platform_available() -> bool {
    // SAFETY: a plain query without arguments.
    unsafe { WebAuthNIsUserVerifyingPlatformAuthenticatorAvailable() }.is_ok_and(|b| b.as_bool())
}

/// Removes a Windows Hello credential (needs WebAuthn API version 4).
pub fn delete_platform_credential(credential_id: &[u8]) -> Result<(), FidoError> {
    // SAFETY: a plain query without arguments.
    if unsafe { WebAuthNGetApiVersionNumber() } < WEBAUTHN_API_VERSION_4 {
        return Err(FidoError::NotSupported);
    }
    // SAFETY: the slice lives for the call.
    unsafe { WebAuthNDeletePlatformCredential(credential_id) }.map_err(|e| map_error(e.code().0, &error_name(e.code())))
}

/// Turns a failed call into a [`FidoError`]: a closed dialog or a timeout is `Cancelled`.
pub fn map_error(code: i32, name: &str) -> FidoError {
    if code == NTE_USER_CANCELLED || code == HRESULT_ERROR_CANCELLED || name == "NotAllowedError" {
        FidoError::Cancelled
    } else {
        FidoError::Failed(format!("{name} (0x{:08X})", code as u32))
    }
}

fn error_name(code: HRESULT) -> String {
    // SAFETY: returns a static string owned by webauthn.dll.
    let name = unsafe { WebAuthNGetErrorName(code) };
    if name.is_null() {
        return String::new();
    }
    // SAFETY: a valid, NUL-terminated wide string.
    unsafe { name.to_string() }.unwrap_or_default()
}

fn failure(e: windows::core::Error) -> FidoError {
    map_error(e.code().0, &error_name(e.code()))
}

fn attachment_flag(attachment: Attachment) -> u32 {
    match attachment {
        Attachment::CrossPlatform => WEBAUTHN_AUTHENTICATOR_ATTACHMENT_CROSS_PLATFORM,
        Attachment::Platform => WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM,
    }
}

fn verification(verify_user: bool) -> u32 {
    if verify_user { WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED } else { WEBAUTHN_USER_VERIFICATION_REQUIREMENT_DISCOURAGED }
}

/// Copies `len` bytes from a pointer owned by webauthn.dll.
///
/// # Safety
/// `ptr` must point to `len` readable bytes (or be null with `len == 0`).
unsafe fn copy(ptr: *const u8, len: u32) -> Vec<u8> {
    if ptr.is_null() || len == 0 {
        return Vec::new();
    }
    // SAFETY: guaranteed by the caller.
    unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec()
}

fn make_credential_blocking(hwnd: isize, application: &str, options: &CreateOptions, client_data: &[u8]) -> Result<Attestation, FidoError> {
    let rp_id = HSTRING::from(application);
    let rp_name = HSTRING::from("SSHCraft");
    let user_name = HSTRING::from(options.user_name.as_str());
    let mut user_id: [u8; 32] = rand::random();
    let mut data = client_data.to_vec();
    let rp = WEBAUTHN_RP_ENTITY_INFORMATION {
        dwVersion: WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
        pwszId: PCWSTR(rp_id.as_ptr()),
        pwszName: PCWSTR(rp_name.as_ptr()),
        pwszIcon: PCWSTR::null(),
    };
    let user = WEBAUTHN_USER_ENTITY_INFORMATION {
        dwVersion: WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
        cbId: user_id.len() as u32,
        pbId: user_id.as_mut_ptr(),
        pwszName: PCWSTR(user_name.as_ptr()),
        pwszIcon: PCWSTR::null(),
        pwszDisplayName: PCWSTR(user_name.as_ptr()),
    };
    let mut parameter = WEBAUTHN_COSE_CREDENTIAL_PARAMETER {
        dwVersion: WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION,
        pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
        lAlg: match options.algorithm {
            SkAlgorithm::EcdsaP256 => WEBAUTHN_COSE_ALGORITHM_ECDSA_P256_WITH_SHA256,
            SkAlgorithm::Ed25519 => COSE_ALGORITHM_EDDSA,
        },
    };
    let parameters = WEBAUTHN_COSE_CREDENTIAL_PARAMETERS { cCredentialParameters: 1, pCredentialParameters: &mut parameter };
    let client = WEBAUTHN_CLIENT_DATA {
        dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
        cbClientDataJSON: data.len() as u32,
        pbClientDataJSON: data.as_mut_ptr(),
        pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
    };
    let make_options = WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS {
        dwVersion: OPTIONS_VERSION,
        dwTimeoutMilliseconds: TIMEOUT_MS,
        dwAuthenticatorAttachment: attachment_flag(options.attachment),
        bRequireResidentKey: false.into(),
        dwUserVerificationRequirement: verification(options.verify_user),
        dwAttestationConveyancePreference: WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE,
        ..Default::default()
    };
    // SAFETY: every pointer above points into locals that live until the call returns.
    let result =
        unsafe { WebAuthNAuthenticatorMakeCredential(HWND(hwnd as *mut core::ffi::c_void), &rp, &user, &parameters, &client, Some(&make_options)) }
            .map_err(failure)?;
    // SAFETY: on success the result is a valid attestation until it is freed below.
    let attestation = unsafe {
        let a = &*result;
        Attestation {
            authenticator_data: copy(a.pbAuthenticatorData, a.cbAuthenticatorData),
            credential_id: copy(a.pbCredentialId, a.cbCredentialId),
        }
    };
    // SAFETY: freed exactly once, after the copy.
    unsafe { WebAuthNFreeCredentialAttestation(Some(result)) };
    Ok(attestation)
}

fn get_assertion_blocking(
    hwnd: isize,
    application: &str,
    credential_id: &[u8],
    client_data: &[u8],
    verify_user: bool,
    attachment: Attachment,
) -> Result<Assertion, FidoError> {
    let rp_id = HSTRING::from(application);
    let mut id = credential_id.to_vec();
    let mut data = client_data.to_vec();
    let mut credential = WEBAUTHN_CREDENTIAL {
        dwVersion: WEBAUTHN_CREDENTIAL_CURRENT_VERSION,
        cbId: id.len() as u32,
        pbId: id.as_mut_ptr(),
        pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
    };
    let client = WEBAUTHN_CLIENT_DATA {
        dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
        cbClientDataJSON: data.len() as u32,
        pbClientDataJSON: data.as_mut_ptr(),
        pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
    };
    let options = WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS {
        dwVersion: OPTIONS_VERSION,
        dwTimeoutMilliseconds: TIMEOUT_MS,
        CredentialList: WEBAUTHN_CREDENTIALS { cCredentials: 1, pCredentials: &mut credential },
        dwAuthenticatorAttachment: attachment_flag(attachment),
        dwUserVerificationRequirement: verification(verify_user),
        ..Default::default()
    };
    // SAFETY: every pointer above points into locals that live until the call returns.
    let result = unsafe { WebAuthNAuthenticatorGetAssertion(HWND(hwnd as *mut core::ffi::c_void), PCWSTR(rp_id.as_ptr()), &client, Some(&options)) }
        .map_err(failure)?;
    // SAFETY: on success the result is a valid assertion until it is freed below.
    let assertion = unsafe {
        let a = &*result;
        Assertion { authenticator_data: copy(a.pbAuthenticatorData, a.cbAuthenticatorData), signature: copy(a.pbSignature, a.cbSignature) }
    };
    // SAFETY: freed exactly once, after the copy.
    unsafe { WebAuthNFreeAssertion(result) };
    Ok(assertion)
}

#[async_trait::async_trait]
impl Authenticator for WebAuthn {
    async fn make_credential(&self, application: &str, options: &CreateOptions, client_data: &[u8]) -> Result<Attestation, FidoError> {
        if !webauthn_available() {
            return Err(FidoError::NotSupported);
        }
        let (hwnd, application, options, client_data) = (self.hwnd, application.to_owned(), options.clone(), client_data.to_vec());
        tokio::task::spawn_blocking(move || make_credential_blocking(hwnd, &application, &options, &client_data))
            .await
            .map_err(|e| FidoError::Failed(e.to_string()))?
    }

    async fn get_assertion(
        &self,
        application: &str,
        credential_id: &[u8],
        client_data: &[u8],
        verify_user: bool,
        attachment: Attachment,
    ) -> Result<Assertion, FidoError> {
        if !webauthn_available() {
            return Err(FidoError::NotSupported);
        }
        let (hwnd, application, credential_id, client_data) = (self.hwnd, application.to_owned(), credential_id.to_vec(), client_data.to_vec());
        tokio::task::spawn_blocking(move || get_assertion_blocking(hwnd, &application, &credential_id, &client_data, verify_user, attachment))
            .await
            .map_err(|e| FidoError::Failed(e.to_string()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_codes_map_to_cancelled() {
        assert_eq!(map_error(NTE_USER_CANCELLED, ""), FidoError::Cancelled);
        assert_eq!(map_error(HRESULT_ERROR_CANCELLED, ""), FidoError::Cancelled);
        assert_eq!(map_error(0x8000_4005_u32 as i32, "NotAllowedError"), FidoError::Cancelled);
        assert_eq!(map_error(0x8009_0027_u32 as i32, "UnknownError"), FidoError::Failed("UnknownError (0x80090027)".into()));
    }

    #[test]
    fn api_version_is_reported() {
        // Windows 10 1903 and later have it; this machine is Windows 11.
        assert!(webauthn_available());
    }
}

//! Windows Hello as a gate in front of the keys stored in Stollen's own vault.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Verified,
    Declined,
    /// Windows Hello is not set up on this PC (or not usable, e.g. over Remote Desktop).
    Unavailable,
}

#[async_trait::async_trait]
pub trait UserPresence: Send + Sync + 'static {
    async fn verify(&self, reason: &str) -> Presence;
}

/// Asks Windows Hello (fingerprint, face or PIN), with the dialog owned by Stollen's window.
pub struct WindowsHello {
    /// Raw handle of the main window, kept as an integer so the struct stays `Send`.
    pub hwnd: isize,
}

#[async_trait::async_trait]
impl UserPresence for WindowsHello {
    async fn verify(&self, reason: &str) -> Presence {
        let hwnd = self.hwnd;
        let reason = reason.to_owned();
        tokio::task::spawn_blocking(move || verify_blocking(hwnd, &reason))
            .await
            .unwrap_or(Presence::Unavailable)
    }
}

#[cfg(windows)]
fn verify_blocking(hwnd: isize, reason: &str) -> Presence {
    use windows::Security::Credentials::UI::{
        UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
    };
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
    use windows::core::{HSTRING, factory};
    use windows_future::IAsyncOperation;

    let available = UserConsentVerifier::CheckAvailabilityAsync()
        .and_then(|op| op.join())
        .is_ok_and(|a| a == UserConsentVerifierAvailability::Available);
    if !available {
        return Presence::Unavailable;
    }
    let result =
        factory::<UserConsentVerifier, IUserConsentVerifierInterop>().and_then(|interop| {
            // SAFETY: `hwnd` is the handle of Stollen's own main window, valid while the app runs.
            let op: IAsyncOperation<UserConsentVerificationResult> = unsafe {
                interop.RequestVerificationForWindowAsync(
                    HWND(hwnd as *mut core::ffi::c_void),
                    &HSTRING::from(reason),
                )?
            };
            op.join()
        });
    match result {
        Ok(UserConsentVerificationResult::Verified) => Presence::Verified,
        Ok(UserConsentVerificationResult::DeviceNotPresent)
        | Ok(UserConsentVerificationResult::NotConfiguredForUser)
        | Ok(UserConsentVerificationResult::DisabledByPolicy) => Presence::Unavailable,
        _ => Presence::Declined,
    }
}

#[cfg(not(windows))]
fn verify_blocking(_hwnd: isize, _reason: &str) -> Presence {
    Presence::Unavailable
}

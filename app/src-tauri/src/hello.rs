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
            // A crashed check must not open the gate.
            .unwrap_or(Presence::Declined)
    }
}

#[cfg(windows)]
fn verify_blocking(hwnd: isize, reason: &str) -> Presence {
    use windows::Security::Credentials::UI::{UserConsentVerificationResult, UserConsentVerifier};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
    use windows::core::{HSTRING, factory};
    use windows_future::IAsyncOperation;

    let availability = UserConsentVerifier::CheckAvailabilityAsync().and_then(|op| op.join());
    if let Some(presence) = gate_from_availability(availability) {
        return presence;
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
    presence_from_result(result)
}

/// `None` when Windows Hello is ready to ask. Only "not set up" opens the gate without a
/// prompt; a busy device or a failed check counts as declined, so it can be retried.
#[cfg(windows)]
fn gate_from_availability(
    availability: windows::core::Result<
        windows::Security::Credentials::UI::UserConsentVerifierAvailability,
    >,
) -> Option<Presence> {
    use windows::Security::Credentials::UI::UserConsentVerifierAvailability as A;
    match availability {
        Ok(A::Available) => None,
        Ok(A::DeviceNotPresent | A::NotConfiguredForUser | A::DisabledByPolicy) => {
            Some(Presence::Unavailable)
        }
        _ => Some(Presence::Declined),
    }
}

#[cfg(windows)]
fn presence_from_result(
    result: windows::core::Result<
        windows::Security::Credentials::UI::UserConsentVerificationResult,
    >,
) -> Presence {
    use windows::Security::Credentials::UI::UserConsentVerificationResult as R;
    match result {
        Ok(R::Verified) => Presence::Verified,
        Ok(R::DeviceNotPresent | R::NotConfiguredForUser | R::DisabledByPolicy) => {
            Presence::Unavailable
        }
        _ => Presence::Declined,
    }
}

#[cfg(not(windows))]
fn verify_blocking(_hwnd: isize, _reason: &str) -> Presence {
    Presence::Unavailable
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows::Security::Credentials::UI::{
        UserConsentVerificationResult as R, UserConsentVerifierAvailability as A,
    };

    fn err<T>() -> windows::core::Result<T> {
        Err(windows::core::Error::from_hresult(windows::core::HRESULT(
            -1,
        )))
    }

    #[test]
    fn available_lets_the_prompt_run() {
        assert_eq!(gate_from_availability(Ok(A::Available)), None);
    }

    #[test]
    fn missing_hello_counts_as_unavailable() {
        for a in [
            A::DeviceNotPresent,
            A::NotConfiguredForUser,
            A::DisabledByPolicy,
        ] {
            assert_eq!(gate_from_availability(Ok(a)), Some(Presence::Unavailable));
        }
    }

    #[test]
    fn busy_device_or_error_counts_as_declined() {
        assert_eq!(
            gate_from_availability(Ok(A::DeviceBusy)),
            Some(Presence::Declined)
        );
        assert_eq!(gate_from_availability(err()), Some(Presence::Declined));
    }

    #[test]
    fn only_verified_passes() {
        assert_eq!(presence_from_result(Ok(R::Verified)), Presence::Verified);
        assert_eq!(presence_from_result(Ok(R::Canceled)), Presence::Declined);
        assert_eq!(presence_from_result(Ok(R::DeviceBusy)), Presence::Declined);
        assert_eq!(presence_from_result(err()), Presence::Declined);
        assert_eq!(
            presence_from_result(Ok(R::NotConfiguredForUser)),
            Presence::Unavailable
        );
    }
}

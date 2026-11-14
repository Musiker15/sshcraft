//! Windows Hello as a gate in front of the keys stored in SSHCraft's own vault.
//!
//! Moved here from the Tauri app (tag `tauri-final`, `app/src-tauri/src/hello.rs`); the engine
//! uses it from R2 on.

// The Windows API is only reachable through FFI. This module is one of the two places in the
// workspace that may use unsafe (AGENTS.md, "Never crash"); every block states why it is sound.
#![allow(unsafe_code)]

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

/// Asks Windows Hello (fingerprint, face or PIN), with the dialog owned by SSHCraft's window.
pub struct WindowsHello {
    /// Raw handle of the main window, kept as an integer so the struct stays `Send`.
    hwnd: isize,
}

impl WindowsHello {
    /// Windows Hello with its dialog owned by the window `hwnd`. Pass the handle of the app's
    /// own top-level window; Windows checks the handle and answers a wrong one with an error,
    /// which counts as declined.
    pub fn new(hwnd: isize) -> Self {
        Self { hwnd }
    }
}

/// Something that can be called off, like a pending Windows Hello request.
pub trait Cancellable {
    fn cancel(&self);
}

/// Cancels the wrapped operation when dropped, unless it was disarmed after finishing.
/// Dropping the `verify` future (the tab was closed) thus also closes the Hello dialog.
pub struct CancelOnDrop<C: Cancellable>(Option<C>);

impl<C: Cancellable> CancelOnDrop<C> {
    pub fn new(operation: C) -> Self {
        Self(Some(operation))
    }

    pub fn disarm(mut self) {
        self.0 = None;
    }
}

impl<C: Cancellable> Drop for CancelOnDrop<C> {
    fn drop(&mut self) {
        if let Some(operation) = self.0.take() {
            operation.cancel();
        }
    }
}

#[async_trait::async_trait]
impl UserPresence for WindowsHello {
    async fn verify(&self, reason: &str) -> Presence {
        verify_async(self.hwnd, reason.to_owned()).await
    }
}

#[cfg(windows)]
impl Cancellable for windows_future::IAsyncOperation<windows::Security::Credentials::UI::UserConsentVerificationResult> {
    fn cancel(&self) {
        let _ = self.Cancel();
    }
}

#[cfg(windows)]
async fn verify_async(hwnd: isize, reason: String) -> Presence {
    use windows::Security::Credentials::UI::UserConsentVerifier;

    let availability = tokio::task::spawn_blocking(|| UserConsentVerifier::CheckAvailabilityAsync().and_then(|op| op.join())).await;
    // A crashed check must not open the gate.
    let Ok(availability) = availability else {
        return Presence::Declined;
    };
    if let Some(presence) = gate_from_availability(availability) {
        return presence;
    }
    let operation = match tokio::task::spawn_blocking(move || request(hwnd, &reason)).await {
        Ok(Ok(operation)) => operation,
        _ => return Presence::Declined,
    };
    let guard = CancelOnDrop::new(operation.clone());
    let result = tokio::task::spawn_blocking(move || operation.join()).await;
    guard.disarm();
    match result {
        Ok(result) => presence_from_result(result),
        Err(_) => Presence::Declined,
    }
}

#[cfg(windows)]
fn request(
    hwnd: isize,
    reason: &str,
) -> windows::core::Result<windows_future::IAsyncOperation<windows::Security::Credentials::UI::UserConsentVerificationResult>> {
    use windows::Security::Credentials::UI::UserConsentVerifier;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
    use windows::core::{HSTRING, factory};

    let interop = factory::<UserConsentVerifier, IUserConsentVerifierInterop>()?;
    // SAFETY: the call only passes the handle value to Windows, which validates it and returns
    // an error for a handle that is not a window; no memory behind the handle is read here. The
    // reason string lives until the call returns.
    unsafe { interop.RequestVerificationForWindowAsync(HWND(hwnd as *mut core::ffi::c_void), &HSTRING::from(reason)) }
}

/// `None` when Windows Hello is ready to ask. Only "not set up" opens the gate without a
/// prompt; a busy device or a failed check counts as declined, so it can be retried.
#[cfg(windows)]
fn gate_from_availability(
    availability: windows::core::Result<windows::Security::Credentials::UI::UserConsentVerifierAvailability>,
) -> Option<Presence> {
    use windows::Security::Credentials::UI::UserConsentVerifierAvailability as A;
    match availability {
        Ok(A::Available) => None,
        Ok(A::DeviceNotPresent | A::NotConfiguredForUser | A::DisabledByPolicy) => Some(Presence::Unavailable),
        _ => Some(Presence::Declined),
    }
}

#[cfg(windows)]
fn presence_from_result(result: windows::core::Result<windows::Security::Credentials::UI::UserConsentVerificationResult>) -> Presence {
    use windows::Security::Credentials::UI::UserConsentVerificationResult as R;
    match result {
        Ok(R::Verified) => Presence::Verified,
        Ok(R::DeviceNotPresent | R::NotConfiguredForUser | R::DisabledByPolicy) => Presence::Unavailable,
        _ => Presence::Declined,
    }
}

/// No presence check outside Windows yet: behaves like a PC without Windows Hello.
#[cfg(not(windows))]
async fn verify_async(_hwnd: isize, _reason: String) -> Presence {
    Presence::Unavailable
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows::Security::Credentials::UI::{UserConsentVerificationResult as R, UserConsentVerifierAvailability as A};

    fn err<T>() -> windows::core::Result<T> {
        Err(windows::core::Error::from_hresult(windows::core::HRESULT(-1)))
    }

    #[test]
    fn available_lets_the_prompt_run() {
        assert_eq!(gate_from_availability(Ok(A::Available)), None);
    }

    #[test]
    fn missing_hello_counts_as_unavailable() {
        for a in [A::DeviceNotPresent, A::NotConfiguredForUser, A::DisabledByPolicy] {
            assert_eq!(gate_from_availability(Ok(a)), Some(Presence::Unavailable));
        }
    }

    #[test]
    fn busy_device_or_error_counts_as_declined() {
        assert_eq!(gate_from_availability(Ok(A::DeviceBusy)), Some(Presence::Declined));
        assert_eq!(gate_from_availability(err()), Some(Presence::Declined));
    }

    #[test]
    fn only_verified_passes() {
        assert_eq!(presence_from_result(Ok(R::Verified)), Presence::Verified);
        assert_eq!(presence_from_result(Ok(R::Canceled)), Presence::Declined);
        assert_eq!(presence_from_result(Ok(R::DeviceBusy)), Presence::Declined);
        assert_eq!(presence_from_result(err()), Presence::Declined);
        assert_eq!(presence_from_result(Ok(R::NotConfiguredForUser)), Presence::Unavailable);
    }
}

#[cfg(test)]
mod cancel_tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone, Default)]
    struct Counter(Arc<AtomicUsize>);

    impl Cancellable for Counter {
        fn cancel(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn dropped_guard_cancels() {
        let counter = Counter::default();
        drop(CancelOnDrop::new(counter.clone()));
        assert_eq!(counter.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn disarmed_guard_does_not_cancel() {
        let counter = Counter::default();
        CancelOnDrop::new(counter.clone()).disarm();
        assert_eq!(counter.0.load(Ordering::SeqCst), 0);
    }
}

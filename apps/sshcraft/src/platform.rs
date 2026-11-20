//! What the engine needs from the window: Windows Hello and security keys show their dialogs
//! owned by SSHCraft's window, whose handle exists only once eframe made the window. The handle
//! is filled in then; until it is, a request answers "unavailable".

use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(not(all(unix, not(target_os = "macos"))))]
use sshcraft_fido::presence::{Presence, UserPresence};

/// The main window's handle as an integer (`0` until the window exists).
#[derive(Clone, Default)]
pub struct WindowHandle(Arc<AtomicIsize>);

impl WindowHandle {
    // Linux and FreeBSD read it only for nothing: their sign-in is polkit, without a window handle.
    #[cfg_attr(all(unix, not(target_os = "macos")), allow(dead_code))]
    pub fn get(&self) -> isize {
        self.0.load(Ordering::SeqCst)
    }

    /// Takes the handle of the window eframe made.
    pub fn take_from(&self, window: &impl HasWindowHandle) {
        if let Ok(handle) = window.window_handle()
            && let RawWindowHandle::Win32(win32) = handle.as_raw()
        {
            self.0.store(win32.hwnd.get(), Ordering::SeqCst);
        }
    }
}

/// Windows Hello, owned by the window once there is one (Linux and FreeBSD ask polkit instead).
#[cfg(not(all(unix, not(target_os = "macos"))))]
pub struct WindowPresence(pub WindowHandle);

#[cfg(not(all(unix, not(target_os = "macos"))))]
#[async_trait::async_trait]
impl UserPresence for WindowPresence {
    async fn verify(&self, reason: &str) -> Presence {
        let hwnd = self.0.get();
        if hwnd == 0 {
            return Presence::Unavailable;
        }
        sshcraft_fido::presence::WindowsHello::new(hwnd).verify(reason).await
    }
}

/// FIDO2 security keys and Windows Hello keys through WebAuthn, owned by the window.
#[cfg(windows)]
pub fn authenticator(window: WindowHandle) -> Option<Arc<dyn sshcraft_fido::Authenticator>> {
    Some(Arc::new(WindowAuthenticator(window)))
}

#[cfg(not(windows))]
pub fn authenticator(_window: WindowHandle) -> Option<Arc<dyn sshcraft_fido::Authenticator>> {
    None
}

#[cfg(windows)]
struct WindowAuthenticator(WindowHandle);

#[cfg(windows)]
#[async_trait::async_trait]
impl sshcraft_fido::Authenticator for WindowAuthenticator {
    async fn make_credential(
        &self,
        application: &str,
        options: &sshcraft_fido::CreateOptions,
        client_data: &[u8],
    ) -> Result<sshcraft_fido::Attestation, sshcraft_fido::FidoError> {
        sshcraft_fido::webauthn::WebAuthn { hwnd: self.0.get() }.make_credential(application, options, client_data).await
    }

    async fn get_assertion(
        &self,
        application: &str,
        credential_id: &[u8],
        client_data: &[u8],
        verify_user: bool,
        attachment: sshcraft_fido::Attachment,
    ) -> Result<sshcraft_fido::Assertion, sshcraft_fido::FidoError> {
        sshcraft_fido::webauthn::WebAuthn { hwnd: self.0.get() }.get_assertion(application, credential_id, client_data, verify_user, attachment).await
    }
}

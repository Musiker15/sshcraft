//! The local connection between the `sshcraft` tool and the running app. Only processes of the
//! same user can open it, and the tool checks that the app on the other end runs as that user
//! too, so another account on the machine can neither listen in nor pretend to be SSHCraft.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::{ClientStream, Listener, ServerStream, connect_to, endpoint, listen_at};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use self::unix::{ClientStream, Listener, ServerStream, connect_to, endpoint, listen_at};

/// Listens on this user's endpoint.
pub fn listen() -> std::io::Result<Listener> {
    listen_at(&endpoint()?)
}

/// Connects to the app of this user; fails when it does not run or is not this user's.
pub async fn connect() -> std::io::Result<ClientStream> {
    connect_to(&endpoint()?).await
}

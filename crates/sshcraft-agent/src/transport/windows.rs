//! A named pipe whose DACL lets only the current user in, that refuses remote clients and that
//! is created as the first instance (so nobody else holds the name already).

use std::ffi::c_void;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::time::Duration;

use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use windows::Win32::Foundation::{CloseHandle, ERROR_PIPE_BUSY, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::core::{HSTRING, PWSTR};

pub type ServerStream = NamedPipeServer;
pub type ClientStream = NamedPipeClient;

fn os(err: windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(err.code().0 & 0xFFFF)
}

/// The SID of the user `process` runs as, as text (`S-1-5-21-…`).
fn process_user_sid(process: HANDLE) -> io::Result<String> {
    // SAFETY: plain Win32 calls on handles owned here; buffers sized as the API reports.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(os)?;
        let result = (|| {
            let mut len = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
            let mut buf = vec![0u8; len as usize];
            GetTokenInformation(
                token,
                TokenUser,
                Some(buf.as_mut_ptr().cast::<c_void>()),
                len,
                &mut len,
            )
            .map_err(os)?;
            let user = &*(buf.as_ptr().cast::<TOKEN_USER>());
            let mut text = PWSTR::null();
            ConvertSidToStringSidW(user.User.Sid, &mut text).map_err(os)?;
            let sid = text
                .to_string()
                .map_err(|e| io::Error::other(e.to_string()));
            let _ = LocalFree(Some(HLOCAL(text.0.cast::<c_void>())));
            sid
        })();
        let _ = CloseHandle(token);
        result
    }
}

fn current_user_sid() -> io::Result<String> {
    // SAFETY: the pseudo handle of the current process needs no closing.
    process_user_sid(unsafe { GetCurrentProcess() })
}

/// `\\.\pipe\sshcraft-agent-<SID>`: one pipe per user.
pub fn endpoint() -> io::Result<String> {
    Ok(format!(r"\\.\pipe\sshcraft-agent-{}", current_user_sid()?))
}

fn create(name: &str, first: bool) -> io::Result<NamedPipeServer> {
    let sddl = format!("D:P(A;;GA;;;{})", current_user_sid()?);
    // SAFETY: the descriptor is created by Windows, used for this one call and freed after it.
    unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from(sddl),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
        .map_err(os)?;
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        let server = ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(name, (&raw mut attributes).cast::<c_void>());
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        server
    }
}

/// Waits for clients on one pipe name; each accepted client gets its own instance.
pub struct Listener {
    name: String,
    next: NamedPipeServer,
}

/// Listens on `name`; fails when the name is taken (by another app, user or instance).
pub fn listen_at(name: &str) -> io::Result<Listener> {
    Ok(Listener {
        name: name.to_owned(),
        next: create(name, true)?,
    })
}

impl Listener {
    pub async fn accept(&mut self) -> io::Result<NamedPipeServer> {
        self.next.connect().await?;
        let next = create(&self.name, false)?;
        Ok(std::mem::replace(&mut self.next, next))
    }
}

/// Connects to `name` and makes sure the process serving it runs as this user.
pub async fn connect_to(name: &str) -> io::Result<NamedPipeClient> {
    let client = loop {
        match ClientOptions::new().open(name) {
            Ok(client) => break client,
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => return Err(e),
        }
    };
    verify_server(&client)?;
    Ok(client)
}

/// The server of the pipe must run as the same user as this process.
fn verify_server(client: &NamedPipeClient) -> io::Result<()> {
    let handle = HANDLE(client.as_raw_handle());
    // SAFETY: the pipe handle stays open for the call; the process handle is closed here.
    let owner = unsafe {
        let mut pid = 0u32;
        GetNamedPipeServerProcessId(handle, &mut pid).map_err(os)?;
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(os)?;
        let sid = process_user_sid(process);
        let _ = CloseHandle(process);
        sid?
    };
    if same_owner(&owner, &current_user_sid()?) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the program on the other end does not run as this user",
        ))
    }
}

fn same_owner(server: &str, me: &str) -> bool {
    server == me
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::{read_message, write_message};
    use tokio::io::BufReader;

    fn test_name() -> String {
        format!(r"\\.\pipe\sshcraft-agent-test-{}", uuid::Uuid::new_v4())
    }

    #[tokio::test]
    async fn a_client_of_the_same_user_gets_through() {
        let name = test_name();
        let mut listener = listen_at(&name).unwrap();
        let server = tokio::spawn(async move {
            let stream = listener.accept().await.unwrap();
            let (read, mut write) = tokio::io::split(stream);
            let mut read = BufReader::new(read);
            let message: serde_json::Value = read_message(&mut read).await.unwrap().unwrap();
            write_message(&mut write, &message).await.unwrap();
        });
        let client = connect_to(&name).await.unwrap();
        let (read, mut write) = tokio::io::split(client);
        write_message(&mut write, &serde_json::json!({"hello": 1}))
            .await
            .unwrap();
        let back: serde_json::Value = read_message(&mut BufReader::new(read))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(back["hello"], 1);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_taken_name_cannot_be_listened_on_again() {
        let name = test_name();
        let _first = listen_at(&name).unwrap();
        assert!(listen_at(&name).is_err());
    }

    #[test]
    fn the_endpoint_names_this_user() {
        let name = endpoint().unwrap();
        assert!(name.starts_with(r"\\.\pipe\sshcraft-agent-S-1-"), "{name}");
        assert!(same_owner("S-1-5-21-1", "S-1-5-21-1"));
        assert!(!same_owner("S-1-5-21-1", "S-1-5-21-2"));
    }

    #[tokio::test]
    async fn nobody_listening_is_an_error() {
        assert!(connect_to(&test_name()).await.is_err());
    }
}

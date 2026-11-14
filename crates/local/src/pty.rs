//! A local shell in a pseudo terminal (ConPTY on Windows): PowerShell, cmd, a WSL distribution
//! or a shell of the user's choosing; elsewhere the login shell.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex, PoisonError};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use sshcraft_model::policy::{Disconnect, ShellEvent};
use tokio::sync::mpsc;

const EVENT_BUFFER: usize = 1024;
const READ_CHUNK: usize = 8 * 1024;

/// What to start, and how it is shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellCommand {
    pub program: String,
    pub args: Vec<String>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LocalError {
    #[error("could not start the shell: {0}")]
    Spawn(String),
    #[error("terminal error: {0}")]
    Io(String),
}

#[cfg(windows)]
fn shell(program: &str, args: &[&str], label: &str) -> ShellCommand {
    ShellCommand { program: program.to_owned(), args: args.iter().map(|a| (*a).to_owned()).collect(), label: label.to_owned() }
}

/// The distributions `wsl.exe --list --quiet` names. It writes UTF-16LE, maybe with a byte order
/// mark; anything that is not that gives no names.
pub fn parse_wsl_list(bytes: &[u8]) -> Vec<String> {
    if !bytes.len().is_multiple_of(2) {
        return Vec::new();
    }
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let Ok(text) = String::from_utf16(&units) else {
        return Vec::new();
    };
    text.trim_start_matches('\u{feff}')
        .lines()
        .map(|l| l.trim_matches(|c: char| c.is_whitespace() || c == '\0'))
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(windows)]
fn in_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

#[cfg(windows)]
fn wsl_distributions() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    // No console window flashing up.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("wsl.exe")
        .args(["--list", "--quiet"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| parse_wsl_list(&out.stdout))
        .unwrap_or_default()
}

/// The shells this device offers, with `custom` (from the settings) last.
pub fn available_shells(custom: Option<ShellCommand>) -> Vec<ShellCommand> {
    let mut shells = Vec::new();
    #[cfg(windows)]
    {
        shells.push(shell("powershell.exe", &["-NoLogo"], "Windows PowerShell"));
        if in_path("pwsh.exe") {
            shells.push(shell("pwsh.exe", &["-NoLogo"], "PowerShell"));
        }
        shells.push(shell("cmd.exe", &[], "Command Prompt"));
        for name in wsl_distributions() {
            shells.push(ShellCommand { program: "wsl.exe".into(), args: vec!["-d".into(), name.clone()], label: format!("WSL: {name}") });
        }
    }
    #[cfg(not(windows))]
    {
        match std::env::var("SHELL").ok().filter(|s| !s.is_empty()) {
            Some(login) => {
                let label = login.rsplit('/').next().unwrap_or(&login).to_owned();
                shells.push(ShellCommand { program: login, args: vec!["-l".into()], label });
            }
            None => shells.push(ShellCommand { program: "/bin/sh".into(), args: Vec::new(), label: "sh".into() }),
        }
    }
    shells.extend(custom);
    shells
}

type Shared<T> = Arc<Mutex<Option<T>>>;

struct Pty {
    /// Taken with the master once the shell has ended.
    writer: Shared<Box<dyn Write + Send>>,
    /// Set once the shell has ended; a write that was under way drops the writer afterwards.
    ended: Arc<std::sync::atomic::AtomicBool>,
    /// Taken (and dropped) once the shell has ended, which ends the reader.
    master: Shared<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

impl Drop for Pty {
    fn drop(&mut self) {
        // The tab is gone: the shell goes with it.
        let _ = self.killer.lock().unwrap_or_else(PoisonError::into_inner).kill();
    }
}

/// The sending half of a local shell. The shell ends when the last clone is dropped.
#[derive(Clone)]
pub struct LocalWriter {
    pty: Arc<Pty>,
}

impl LocalWriter {
    pub fn write(&self, data: &[u8]) -> Result<(), LocalError> {
        let mut slot = self.pty.writer.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(writer) = slot.as_mut() else {
            return Err(LocalError::Io("the shell has ended".into()));
        };
        let written = writer.write_all(data).and_then(|()| writer.flush()).map_err(|e| LocalError::Io(e.to_string()));
        // The waiter could not take the writer while this write held it.
        if self.pty.ended.load(std::sync::atomic::Ordering::SeqCst) {
            slot.take();
        }
        written
    }

    /// Ends the shell now, even while other clones (a write under way) still live.
    pub fn close(&self) {
        let _ = self.pty.killer.lock().unwrap_or_else(PoisonError::into_inner).kill();
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), LocalError> {
        match &*self.pty.master.lock().unwrap_or_else(PoisonError::into_inner) {
            Some(master) => master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| LocalError::Io(e.to_string())),
            None => Ok(()),
        }
    }
}

/// Whether a shell may start in `dir`: an absolute, local directory. A network path (UNC) is
/// refused before anything touches it, since even looking at it connects to that server.
fn usable_cwd(dir: &std::path::Path) -> bool {
    let text = dir.as_os_str().to_string_lossy();
    let network = text.starts_with(r"\\") || text.starts_with("//");
    !network && dir.is_absolute() && dir.is_dir()
}

fn home() -> Option<std::path::PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(std::path::PathBuf::from).filter(|p| p.is_dir())
}

/// Starts the shell in `cwd` when that is a directory, else in the user's home directory, with
/// `TERM` set to `term`. Output comes as [`ShellEvent::Data`], then [`ShellEvent::Ended`] with
/// the exit code once the shell has ended and its output is read.
pub fn spawn(
    cmd: &ShellCommand,
    cols: u16,
    rows: u16,
    term: &str,
    cwd: Option<&std::path::Path>,
) -> Result<(LocalWriter, mpsc::Receiver<ShellEvent>), LocalError> {
    let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| LocalError::Spawn(e.to_string()))?;
    let mut builder = CommandBuilder::new(&cmd.program);
    builder.args(&cmd.args);
    builder.env("TERM", term);
    let dir = cwd.filter(|dir| usable_cwd(dir)).map(std::path::Path::to_path_buf).or_else(home);
    if let Some(dir) = dir {
        builder.cwd(dir);
    }
    let mut child = pair.slave.spawn_command(builder).map_err(|e| LocalError::Spawn(e.to_string()))?;
    // Only the child holds the other end now, so the reader sees the end when it exits.
    drop(pair.slave);
    let mut killer = child.clone_killer();
    let handles = pair.master.try_clone_reader().and_then(|reader| Ok((reader, pair.master.take_writer()?)));
    let (mut reader, writer) = match handles {
        Ok(handles) => handles,
        Err(e) => {
            // No terminal to talk to it: the shell must not keep running unseen.
            let _ = killer.kill();
            return Err(LocalError::Io(e.to_string()));
        }
    };
    let master = Arc::new(Mutex::new(Some(pair.master)));
    let writer = Arc::new(Mutex::new(Some(writer)));
    let ended = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (tx, rx) = mpsc::channel(EVENT_BUFFER);
    let exit: Arc<Mutex<Option<Option<u32>>>> = Arc::new(Mutex::new(None));

    // Waits for the shell; then closes the terminal (both ends this side holds), so the reader
    // gets the rest and the end.
    {
        let master = master.clone();
        let writer = writer.clone();
        let ended = ended.clone();
        let exit = exit.clone();
        std::thread::spawn(move || {
            let code = child.wait().ok().map(|status| status.exit_code());
            *exit.lock().unwrap_or_else(PoisonError::into_inner) = Some(code);
            ended.store(true, std::sync::atomic::Ordering::SeqCst);
            // A write under way holds the writer; it drops it itself when done.
            if let Ok(mut slot) = writer.try_lock() {
                slot.take();
            }
            master.lock().unwrap_or_else(PoisonError::into_inner).take();
        });
    }
    std::thread::spawn(move || {
        let mut buf = vec![0u8; READ_CHUNK];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.blocking_send(ShellEvent::Data(buf[..n].to_vec())).is_err() {
                        return;
                    }
                }
            }
        }
        // The reader can end a moment before the waiter has the code.
        let mut code = None;
        for _ in 0..200 {
            if let Some(c) = *exit.lock().unwrap_or_else(PoisonError::into_inner) {
                code = Some(c);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let reason = match code {
            Some(code) => Disconnect::Exited(code),
            None => Disconnect::Lost("terminal closed".into()),
        };
        let _ = tx.blocking_send(ShellEvent::Ended(reason));
    });

    Ok((LocalWriter { pty: Arc::new(Pty { writer, ended, master, killer: Mutex::new(killer) }) }, rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands in for the shell's killer: there is no shell in these tests.
    #[derive(Debug)]
    struct NoShell;

    impl ChildKiller for NoShell {
        fn kill(&mut self) -> std::io::Result<()> {
            Ok(())
        }
        fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
            Box::new(NoShell)
        }
    }

    fn poison<T>(lock: &Mutex<T>) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = lock.lock();
            panic!("a thread dies while it holds the lock");
        }));
        assert!(lock.is_poisoned());
    }

    /// A thread that panicked while holding the writer, the terminal or the killer must not
    /// take the next write, resize, close or the closing tab down.
    #[test]
    fn poisoned_writer_lock_does_not_panic() {
        let writer: Shared<Box<dyn Write + Send>> = Arc::new(Mutex::new(Some(Box::new(Vec::<u8>::new()))));
        let master: Shared<Box<dyn MasterPty + Send>> = Arc::new(Mutex::new(None));
        let local = LocalWriter {
            pty: Arc::new(Pty { writer: writer.clone(), ended: Arc::default(), master: master.clone(), killer: Mutex::new(Box::new(NoShell)) }),
        };
        poison(&writer);
        poison(&master);
        poison(&local.pty.killer);
        assert!(local.write(b"ls\r").is_ok());
        assert!(local.resize(80, 24).is_ok());
        local.close();
        drop(local);
    }

    #[test]
    fn only_local_absolute_directories_are_used() {
        assert!(usable_cwd(&std::env::temp_dir()));
        for bad in [r"\\attacker\share", "//attacker/share", r"\\?\UNC\attacker\share", "relative/dir", "."] {
            assert!(!usable_cwd(std::path::Path::new(bad)), "{bad}");
        }
    }

    fn utf16(text: &str, bom: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if bom {
            out.extend(0xfeffu16.to_le_bytes());
        }
        for unit in text.encode_utf16() {
            out.extend(unit.to_le_bytes());
        }
        out
    }

    #[test]
    fn parse_wsl_list_utf16_with_bom() {
        assert_eq!(parse_wsl_list(&utf16("Ubuntu\r\n\r\nDebian\r\n", true)), vec!["Ubuntu".to_owned(), "Debian".to_owned()]);
    }

    #[test]
    fn parse_wsl_list_without_bom() {
        assert_eq!(parse_wsl_list(&utf16("Ubuntu-24.04\r\n", false)), vec!["Ubuntu-24.04".to_owned()]);
    }

    #[test]
    fn parse_wsl_list_garbage_is_empty() {
        assert!(parse_wsl_list(&[0x41, 0x00, 0x42]).is_empty());
        assert!(parse_wsl_list(&[]).is_empty());
    }
}

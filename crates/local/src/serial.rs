//! A serial port as a terminal: baud rate, data bits, stop bits, parity and flow control.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sshcraft_model::policy::{Disconnect, ShellEvent};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Parity {
    None,
    Odd,
    Even,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowControl {
    None,
    Software,
    Hardware,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerialSettings {
    pub port: String,
    pub baud: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: Parity,
    pub flow: FlowControl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SerialPortInfo {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SerialError {
    #[error("invalid serial setting: {0}")]
    Invalid(&'static str),
    #[error("could not open the serial port: {0}")]
    Open(String),
}

/// Baud rates the settings accept.
pub const MIN_BAUD: u32 = 50;
pub const MAX_BAUD: u32 = 4_000_000;
const EVENT_BUFFER: usize = 1024;
const READ_CHUNK: usize = 4096;
/// How long one read waits; a timeout is not an error, it only lets the reader look up.
const READ_TIMEOUT: Duration = Duration::from_millis(200);
/// This many empty reads in a row mean the port is gone (about a second).
const MAX_EMPTY_READS: u32 = 50;
const EMPTY_PAUSE: Duration = Duration::from_millis(20);

impl Default for SerialSettings {
    /// 115200 baud, 8 data bits, no parity, 1 stop bit, no flow control.
    fn default() -> Self {
        Self { port: String::new(), baud: 115_200, data_bits: 8, stop_bits: 1, parity: Parity::None, flow: FlowControl::None }
    }
}

impl SerialSettings {
    pub fn validate(&self) -> Result<(), SerialError> {
        if self.port.trim().is_empty() {
            return Err(SerialError::Invalid("port"));
        }
        if !(MIN_BAUD..=MAX_BAUD).contains(&self.baud) {
            return Err(SerialError::Invalid("baud"));
        }
        if !(5..=8).contains(&self.data_bits) {
            return Err(SerialError::Invalid("data_bits"));
        }
        if !(1..=2).contains(&self.stop_bits) {
            return Err(SerialError::Invalid("stop_bits"));
        }
        Ok(())
    }
}

/// The serial ports of this device; none when they cannot be listed. serial2 lists paths only
/// (`COM3`, `/dev/ttyUSB0`), so the description stays empty.
pub fn list_ports() -> Vec<SerialPortInfo> {
    serial2::SerialPort::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|path| SerialPortInfo { name: path.to_string_lossy().into_owned(), description: String::new() })
        .collect()
}

/// The settings as serial2 takes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Serial2Values {
    pub(crate) baud: u32,
    pub(crate) char_size: serial2::CharSize,
    pub(crate) stop_bits: serial2::StopBits,
    pub(crate) parity: serial2::Parity,
    pub(crate) flow: serial2::FlowControl,
}

/// The serial2 values for the settings. A value outside what the settings allow is an error,
/// never a silent default.
pub(crate) fn to_serial2(settings: &SerialSettings) -> Result<Serial2Values, SerialError> {
    settings.validate()?;
    let char_size = match settings.data_bits {
        5 => serial2::CharSize::Bits5,
        6 => serial2::CharSize::Bits6,
        7 => serial2::CharSize::Bits7,
        8 => serial2::CharSize::Bits8,
        _ => return Err(SerialError::Invalid("data_bits")),
    };
    let stop_bits = match settings.stop_bits {
        1 => serial2::StopBits::One,
        2 => serial2::StopBits::Two,
        _ => return Err(SerialError::Invalid("stop_bits")),
    };
    let parity = match settings.parity {
        Parity::None => serial2::Parity::None,
        Parity::Odd => serial2::Parity::Odd,
        Parity::Even => serial2::Parity::Even,
    };
    let flow = match settings.flow {
        FlowControl::None => serial2::FlowControl::None,
        FlowControl::Software => serial2::FlowControl::XonXoff,
        FlowControl::Hardware => serial2::FlowControl::RtsCts,
    };
    Ok(Serial2Values { baud: settings.baud, char_size, stop_bits, parity, flow })
}

/// Reads until the port fails: timeouts are waited out, anything else ends the session as lost
/// (an adapter pulled out, a port gone).
pub(crate) fn pump<R: Read + Send + 'static>(mut reader: R, tx: mpsc::Sender<ShellEvent>) {
    let mut buf = vec![0u8; READ_CHUNK];
    let mut empty = 0;
    loop {
        match reader.read(&mut buf) {
            // A port that keeps answering with nothing (end of file after a hang-up on some
            // systems) is gone; a short pause keeps this from spinning meanwhile.
            Ok(0) => {
                empty += 1;
                if empty >= MAX_EMPTY_READS {
                    let _ = tx.blocking_send(ShellEvent::Ended(Disconnect::Lost("the port closed".into())));
                    return;
                }
                std::thread::sleep(EMPTY_PAUSE);
                continue;
            }
            Ok(n) => {
                empty = 0;
                if tx.blocking_send(ShellEvent::Data(buf[..n].to_vec())).is_err() {
                    return;
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) => {}
            Err(e) => {
                let _ = tx.blocking_send(ShellEvent::Ended(Disconnect::Lost(e.to_string())));
                return;
            }
        }
        if tx.is_closed() {
            return;
        }
    }
}

/// The sending half of an open serial port. The port closes when the last clone is dropped
/// and the reader notices the closed tab.
#[derive(Clone)]
pub struct SerialWriter {
    port: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl SerialWriter {
    pub fn write(&self, data: &[u8]) -> Result<(), SerialError> {
        let mut port = self.port.lock().unwrap_or_else(PoisonError::into_inner);
        port.write_all(data).and_then(|()| port.flush()).map_err(|e| SerialError::Open(e.to_string()))
    }
}

/// Opens the port with the settings; its output comes as [`ShellEvent::Data`].
pub fn open(settings: &SerialSettings) -> Result<(SerialWriter, mpsc::Receiver<ShellEvent>), SerialError> {
    let values = to_serial2(settings)?;
    let mut port = serial2::SerialPort::open(settings.port.trim(), move |mut port_settings: serial2::Settings| {
        port_settings.set_raw();
        port_settings.set_baud_rate(values.baud)?;
        port_settings.set_char_size(values.char_size);
        port_settings.set_stop_bits(values.stop_bits);
        port_settings.set_parity(values.parity);
        port_settings.set_flow_control(values.flow);
        Ok(port_settings)
    })
    .map_err(|e| SerialError::Open(e.to_string()))?;
    port.set_read_timeout(READ_TIMEOUT).map_err(|e| SerialError::Open(e.to_string()))?;
    let reader = port.try_clone().map_err(|e| SerialError::Open(e.to_string()))?;
    let (tx, rx) = mpsc::channel(EVENT_BUFFER);
    std::thread::spawn(move || pump(reader, tx));
    Ok((SerialWriter { port: Arc::new(Mutex::new(Box::new(port) as Box<dyn Write + Send>)) }, rx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshcraft_model::policy::Disconnect;
    use std::collections::VecDeque;

    #[test]
    fn default_is_115200_8n1() {
        let d = SerialSettings::default();
        assert_eq!((d.baud, d.data_bits, d.stop_bits, d.parity, d.flow), (115_200, 8, 1, Parity::None, FlowControl::None));
        assert!(d.port.is_empty());
    }

    #[test]
    fn validate_rejects_bad_values() {
        let ok = SerialSettings { port: "COM3".into(), ..SerialSettings::default() };
        assert!(ok.validate().is_ok());
        for bad in [
            SerialSettings { baud: 0, ..ok.clone() },
            SerialSettings { baud: 49, ..ok.clone() },
            SerialSettings { baud: 4_000_001, ..ok.clone() },
            SerialSettings { data_bits: 9, ..ok.clone() },
            SerialSettings { data_bits: 4, ..ok.clone() },
            SerialSettings { stop_bits: 3, ..ok.clone() },
            SerialSettings { port: "  ".into(), ..ok.clone() },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        for fine in [5, 6, 7, 8] {
            assert!(SerialSettings { data_bits: fine, ..ok.clone() }.validate().is_ok());
        }
        assert!(SerialSettings { stop_bits: 2, baud: 50, ..ok.clone() }.validate().is_ok());
    }

    /// Hands out prepared reads, then fails like an unplugged adapter.
    struct Script(VecDeque<std::io::Result<Vec<u8>>>);

    impl Read for Script {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.0.pop_front() {
                Some(Ok(bytes)) => {
                    buf[..bytes.len()].copy_from_slice(&bytes);
                    Ok(bytes.len())
                }
                Some(Err(e)) => Err(e),
                None => Err(std::io::ErrorKind::BrokenPipe.into()),
            }
        }
    }

    #[tokio::test]
    async fn pump_forwards_then_reports_lost() {
        let (tx, mut rx) = mpsc::channel(16);
        let script = Script(VecDeque::from(vec![
            Ok(b"one".to_vec()),
            Err(std::io::ErrorKind::TimedOut.into()),
            Ok(b"two".to_vec()),
            Err(std::io::ErrorKind::BrokenPipe.into()),
        ]));
        std::thread::spawn(move || pump(script, tx));
        let mut got = Vec::new();
        while let Some(event) = rx.recv().await {
            got.push(event);
        }
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], ShellEvent::Data(b"one".to_vec()));
        assert_eq!(got[1], ShellEvent::Data(b"two".to_vec()));
        assert!(matches!(got[2], ShellEvent::Ended(Disconnect::Lost(_))));
    }

    /// Reads that always return nothing, like a port that reports end of file after a hang-up.
    struct Empty;

    impl Read for Empty {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Ok(0)
        }
    }

    #[tokio::test]
    async fn endless_empty_reads_end_as_lost() {
        let (tx, mut rx) = mpsc::channel(16);
        std::thread::spawn(move || pump(Empty, tx));
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await.expect("the pump must not spin forever");
        assert!(matches!(event, Some(ShellEvent::Ended(Disconnect::Lost(_)))));
    }

    fn port(settings: SerialSettings) -> SerialSettings {
        SerialSettings { port: "COM3".into(), baud: 9600, ..settings }
    }

    /// Every value of the settings reaches serial2 as it was set; none falls back to a default.
    #[test]
    fn settings_map_to_serial2() {
        use serial2::{CharSize, FlowControl as Flow, Parity as Par, StopBits};
        let base = port(SerialSettings::default());
        for (bits, size) in [(5, CharSize::Bits5), (6, CharSize::Bits6), (7, CharSize::Bits7), (8, CharSize::Bits8)] {
            let values = to_serial2(&SerialSettings { data_bits: bits, ..base.clone() }).unwrap();
            assert_eq!(values.char_size, size, "{bits} data bits");
            assert_eq!(values.baud, 9600);
        }
        for (stop, want) in [(1, StopBits::One), (2, StopBits::Two)] {
            let values = to_serial2(&SerialSettings { stop_bits: stop, ..base.clone() }).unwrap();
            assert_eq!(values.stop_bits, want, "{stop} stop bits");
        }
        for (parity, want) in [(Parity::None, Par::None), (Parity::Odd, Par::Odd), (Parity::Even, Par::Even)] {
            let values = to_serial2(&SerialSettings { parity, ..base.clone() }).unwrap();
            assert_eq!(values.parity, want, "{parity:?}");
        }
        for (flow, want) in [(FlowControl::None, Flow::None), (FlowControl::Software, Flow::XonXoff), (FlowControl::Hardware, Flow::RtsCts)] {
            let values = to_serial2(&SerialSettings { flow, ..base.clone() }).unwrap();
            assert_eq!(values.flow, want, "{flow:?}");
        }
    }

    /// A value the user interface should never send is an error, not a silent 8 data bits.
    #[test]
    fn invalid_data_bits_are_an_error() {
        for bits in [4, 9] {
            let settings = port(SerialSettings { data_bits: bits, ..SerialSettings::default() });
            assert_eq!(to_serial2(&settings), Err(SerialError::Invalid("data_bits")), "{bits}");
        }
    }

    /// The same for stop bits: neither 0 nor 3 becomes one stop bit.
    #[test]
    fn invalid_stop_bits_are_an_error() {
        for stop in [0, 3] {
            let settings = port(SerialSettings { stop_bits: stop, ..SerialSettings::default() });
            assert_eq!(to_serial2(&settings), Err(SerialError::Invalid("stop_bits")), "{stop}");
        }
    }

    /// A writer thread that panicked while holding the port must not take the next write down.
    #[test]
    fn poisoned_port_lock_does_not_panic() {
        let writer = SerialWriter { port: Arc::new(Mutex::new(Box::new(Vec::<u8>::new()) as Box<dyn Write + Send>)) };
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = writer.port.lock();
            panic!("a writer dies while it holds the port");
        }));
        assert!(writer.port.is_poisoned());
        assert!(writer.write(b"AT\r").is_ok());
    }

    #[test]
    fn settings_json_roundtrip() {
        let s = SerialSettings { port: "/dev/ttyUSB0".into(), parity: Parity::Even, flow: FlowControl::Hardware, ..SerialSettings::default() };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"parity\":\"even\""), "{json}");
        assert_eq!(serde_json::from_str::<SerialSettings>(&json).unwrap(), s);
    }
}

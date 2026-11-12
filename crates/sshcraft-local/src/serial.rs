//! A serial port as a terminal: baud rate, data bits, stop bits, parity and flow control.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sshcraft_core::policy::{Disconnect, ShellEvent};
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
        Self {
            port: String::new(),
            baud: 115_200,
            data_bits: 8,
            stop_bits: 1,
            parity: Parity::None,
            flow: FlowControl::None,
        }
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

/// The serial ports of this device; none when they cannot be listed.
pub fn list_ports() -> Vec<SerialPortInfo> {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let description = match p.port_type {
                serialport::SerialPortType::UsbPort(usb) => usb
                    .product
                    .or(usb.manufacturer)
                    .unwrap_or_else(|| "USB".into()),
                serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
                serialport::SerialPortType::PciPort => "PCI".into(),
                serialport::SerialPortType::Unknown => String::new(),
            };
            SerialPortInfo {
                name: p.port_name,
                description,
            }
        })
        .collect()
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
                    let _ = tx.blocking_send(ShellEvent::Ended(Disconnect::Lost(
                        "the port closed".into(),
                    )));
                    return;
                }
                std::thread::sleep(EMPTY_PAUSE);
                continue;
            }
            Ok(n) => {
                empty = 0;
                if tx
                    .blocking_send(ShellEvent::Data(buf[..n].to_vec()))
                    .is_err()
                {
                    return;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted
                ) => {}
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
    port: Arc<Mutex<Box<dyn serialport::SerialPort>>>,
}

impl SerialWriter {
    pub fn write(&self, data: &[u8]) -> Result<(), SerialError> {
        let mut port = self.port.lock().expect("port lock");
        port.write_all(data)
            .and_then(|()| port.flush())
            .map_err(|e| SerialError::Open(e.to_string()))
    }
}

/// Opens the port with the settings; its output comes as [`ShellEvent::Data`].
pub fn open(
    settings: &SerialSettings,
) -> Result<(SerialWriter, mpsc::Receiver<ShellEvent>), SerialError> {
    settings.validate()?;
    let data_bits = match settings.data_bits {
        5 => serialport::DataBits::Five,
        6 => serialport::DataBits::Six,
        7 => serialport::DataBits::Seven,
        _ => serialport::DataBits::Eight,
    };
    let stop_bits = match settings.stop_bits {
        2 => serialport::StopBits::Two,
        _ => serialport::StopBits::One,
    };
    let parity = match settings.parity {
        Parity::None => serialport::Parity::None,
        Parity::Odd => serialport::Parity::Odd,
        Parity::Even => serialport::Parity::Even,
    };
    let flow = match settings.flow {
        FlowControl::None => serialport::FlowControl::None,
        FlowControl::Software => serialport::FlowControl::Software,
        FlowControl::Hardware => serialport::FlowControl::Hardware,
    };
    let port = serialport::new(settings.port.trim(), settings.baud)
        .data_bits(data_bits)
        .stop_bits(stop_bits)
        .parity(parity)
        .flow_control(flow)
        .timeout(READ_TIMEOUT)
        .open()
        .map_err(|e| SerialError::Open(e.to_string()))?;
    let reader = port
        .try_clone()
        .map_err(|e| SerialError::Open(e.to_string()))?;
    let (tx, rx) = mpsc::channel(EVENT_BUFFER);
    std::thread::spawn(move || pump(reader, tx));
    Ok((
        SerialWriter {
            port: Arc::new(Mutex::new(port)),
        },
        rx,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshcraft_core::policy::Disconnect;
    use std::collections::VecDeque;

    #[test]
    fn default_is_115200_8n1() {
        let d = SerialSettings::default();
        assert_eq!(
            (d.baud, d.data_bits, d.stop_bits, d.parity, d.flow),
            (115_200, 8, 1, Parity::None, FlowControl::None)
        );
        assert!(d.port.is_empty());
    }

    #[test]
    fn validate_rejects_bad_values() {
        let ok = SerialSettings {
            port: "COM3".into(),
            ..SerialSettings::default()
        };
        assert!(ok.validate().is_ok());
        for bad in [
            SerialSettings {
                baud: 0,
                ..ok.clone()
            },
            SerialSettings {
                baud: 49,
                ..ok.clone()
            },
            SerialSettings {
                baud: 4_000_001,
                ..ok.clone()
            },
            SerialSettings {
                data_bits: 9,
                ..ok.clone()
            },
            SerialSettings {
                data_bits: 4,
                ..ok.clone()
            },
            SerialSettings {
                stop_bits: 3,
                ..ok.clone()
            },
            SerialSettings {
                port: "  ".into(),
                ..ok.clone()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        for fine in [5, 6, 7, 8] {
            assert!(
                SerialSettings {
                    data_bits: fine,
                    ..ok.clone()
                }
                .validate()
                .is_ok()
            );
        }
        assert!(
            SerialSettings {
                stop_bits: 2,
                baud: 50,
                ..ok.clone()
            }
            .validate()
            .is_ok()
        );
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
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("the pump must not spin forever");
        assert!(matches!(
            event,
            Some(ShellEvent::Ended(Disconnect::Lost(_)))
        ));
    }

    #[test]
    fn settings_json_roundtrip() {
        let s = SerialSettings {
            port: "/dev/ttyUSB0".into(),
            parity: Parity::Even,
            flow: FlowControl::Hardware,
            ..SerialSettings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"parity\":\"even\""), "{json}");
        assert_eq!(serde_json::from_str::<SerialSettings>(&json).unwrap(), s);
    }
}

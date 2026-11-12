//! Push notices to connected devices. An event only says that something changed; the device
//! fetches the data over HTTP, so a missed event costs nothing but delay.

use serde::{Deserialize, Serialize};
use sshcraft_proto::area::AreaId;
use sshcraft_proto::identity::AccountPublic;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Queue length per connection; a connection that falls this far behind is dropped.
pub const QUEUE: usize = 256;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    DeviceLog {
        account: AccountPublic,
        len: u64,
    },
    Manifest {
        area_id: AreaId,
        version: u64,
    },
    Envelopes {
        area_id: AreaId,
        key_version: u32,
    },
    Record {
        area_id: AreaId,
        record_id: Uuid,
        version: u64,
        seq: u64,
    },
    AreaDeleted {
        area_id: AreaId,
    },
    SessionEnded,
}

struct Subscription {
    id: u64,
    account: AccountPublic,
    tx: mpsc::Sender<Event>,
}

#[derive(Default)]
pub struct Hub {
    by_device: Mutex<HashMap<Uuid, Vec<Subscription>>>,
    next: AtomicU64,
}

impl Hub {
    pub fn subscribe(
        &self,
        device_id: Uuid,
        account: AccountPublic,
    ) -> (u64, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel(QUEUE);
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut map) = self.by_device.lock() {
            map.entry(device_id)
                .or_default()
                .push(Subscription { id, account, tx });
        }
        (id, rx)
    }

    pub fn unsubscribe(&self, device_id: Uuid, subscription: u64) {
        let Ok(mut map) = self.by_device.lock() else {
            return;
        };
        if let Some(subs) = map.get_mut(&device_id) {
            subs.retain(|s| s.id != subscription);
            if subs.is_empty() {
                map.remove(&device_id);
            }
        }
    }

    /// Sends `event` to every connection of the given accounts. A connection whose queue is
    /// full or closed is dropped; its device resyncs when it reconnects.
    pub fn publish(&self, accounts: &[AccountPublic], event: Event) {
        let Ok(mut map) = self.by_device.lock() else {
            return;
        };
        for subs in map.values_mut() {
            subs.retain(|s| !accounts.contains(&s.account) || s.tx.try_send(event.clone()).is_ok());
        }
        map.retain(|_, subs| !subs.is_empty());
    }

    /// Ends every connection of a device: sends `SessionEnded` and drops the subscriptions.
    pub fn end_device(&self, device_id: Uuid) {
        let Ok(mut map) = self.by_device.lock() else {
            return;
        };
        for sub in map.remove(&device_id).unwrap_or_default() {
            let _ = sub.tx.try_send(Event::SessionEnded);
        }
    }
}

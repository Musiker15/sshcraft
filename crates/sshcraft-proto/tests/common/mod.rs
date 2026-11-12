#![allow(dead_code)]

use sshcraft_proto::area::{AreaId, AreaKey, AreaManifest, Member, Role};
use sshcraft_proto::devicelog::{DeviceLog, DeviceSet};
use sshcraft_proto::identity::{AccountKey, AccountPublic, DeviceCertificate, DeviceKeys};
use sshcraft_proto::record::{RecordHeader, SealedRecord};
use uuid::Uuid;

pub struct Device {
    pub id: Uuid,
    pub keys: DeviceKeys,
}

pub struct Person {
    pub account: AccountKey,
    pub log: DeviceLog,
    pub devices: Vec<Device>,
}

impl Person {
    pub fn new(devices: usize) -> Self {
        let account = AccountKey::generate();
        let mut log = DeviceLog::default();
        let devices = (0..devices)
            .map(|i| {
                let keys = DeviceKeys::generate();
                let id = Uuid::new_v4();
                let cert = DeviceCertificate::issue(
                    &account,
                    id,
                    keys.public(),
                    &format!("d{i}"),
                    "windows",
                    1,
                );
                log.append_add(&account, cert, 1);
                Device { id, keys }
            })
            .collect();
        Self {
            account,
            log,
            devices,
        }
    }

    pub fn public(&self) -> AccountPublic {
        self.account.public()
    }

    pub fn set(&self) -> DeviceSet {
        self.log.verify(&self.public()).unwrap()
    }

    pub fn revoke(&mut self, target: usize) {
        let id = self.devices[target].id;
        self.log.append_revoke_by_account(&self.account, id, 2);
    }
}

/// An owner with two devices, a writer and a reader member, a stranger, and one shared area.
pub struct World {
    pub owner: Person,
    pub writer: Person,
    pub reader: Person,
    pub stranger: Person,
    pub area: AreaId,
    pub key: AreaKey,
    pub manifest: AreaManifest,
}

impl World {
    pub fn new() -> Self {
        let owner = Person::new(2);
        let writer = Person::new(1);
        let reader = Person::new(1);
        let stranger = Person::new(1);
        let area = AreaId(Uuid::new_v4());
        let manifest = AreaManifest::sign(
            area,
            1,
            1,
            owner.public(),
            vec![
                Member {
                    account: writer.public(),
                    role: Role::Write,
                },
                Member {
                    account: reader.public(),
                    role: Role::Read,
                },
            ],
            owner.devices[0].id,
            &owner.devices[0].keys,
        );
        Self {
            owner,
            writer,
            reader,
            stranger,
            area,
            key: AreaKey::generate(),
            manifest,
        }
    }

    pub fn header(&self, by: &Person, device: usize, version: u64) -> RecordHeader {
        RecordHeader {
            record_id: Uuid::new_v4(),
            area_id: self.area,
            version,
            key_version: self.manifest.key_version,
            author_account: by.public(),
            author_device: by.devices[device].id,
        }
    }

    pub fn seal(&self, by: &Person, device: usize, body: &[u8]) -> SealedRecord {
        SealedRecord::seal(
            self.header(by, device, 1),
            body,
            &self.key,
            &by.devices[device].keys,
        )
    }
}

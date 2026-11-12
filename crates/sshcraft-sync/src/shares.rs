//! Shares: an area of this account with members, holding a group with all its hosts, or copies
//! of a selection of hosts or of one host (design, section 8).

use serde::{Deserialize, Serialize};
use sshcraft_core::store::{ShareKind, copy_id};
use uuid::Uuid;

/// What a share holds, written by the owner into the share's area, so members and the owner's
/// other devices know which rules apply (a selection may only be changed, not added to).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareInfo {
    pub kind: ShareKind,
    /// The shared group on the owner's devices.
    pub group: Option<String>,
    pub with_keys: bool,
}

#[derive(Serialize, Deserialize)]
struct ShareInfoBody {
    share_info: ShareInfo,
}

/// The record id of a share's info record.
pub fn share_info_record_id(area: Uuid) -> Uuid {
    copy_id(area, area)
}

#[allow(dead_code)] // used by Task 6
pub(crate) fn encode_share_info(info: &ShareInfo) -> Vec<u8> {
    serde_json::to_vec(&ShareInfoBody {
        share_info: info.clone(),
    })
    .expect("share info serializes")
}

/// The share info a record body holds, if it is one.
pub(crate) fn decode_share_info(body: &[u8]) -> Option<ShareInfo> {
    serde_json::from_slice::<ShareInfoBody>(body)
        .ok()
        .map(|b| b.share_info)
}

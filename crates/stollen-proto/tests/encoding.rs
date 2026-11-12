use serde::{Deserialize, Serialize};
use stollen_proto::encoding::b64;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Holder {
    #[serde(with = "b64")]
    bytes: Vec<u8>,
    #[serde(with = "b64")]
    fixed: [u8; 4],
}

#[test]
fn b64_roundtrip_in_json() {
    let value = Holder {
        bytes: vec![0, 255, 7, 8, 9],
        fixed: [1, 2, 3, 4],
    };
    let json = serde_json::to_string(&value).unwrap();
    assert!(!json.contains('='), "{json}");
    assert_eq!(serde_json::from_str::<Holder>(&json).unwrap(), value);
}

#[test]
fn b64_rejects_wrong_length_for_fixed_arrays() {
    let json = r#"{"bytes":"AA","fixed":"AAEC"}"#;
    assert!(serde_json::from_str::<Holder>(json).is_err());
}

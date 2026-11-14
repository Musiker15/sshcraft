use super::*;
use sshcraft_model::model::{ConnectionSettings, Group, GroupId, Host, HostId, HostOs, KeyRef};
use sshcraft_store::store::Store;
use sshcraft_store::vault::{MemoryMasterKey, Vault};

fn group(name: &str, parent: Option<GroupId>, address: Option<&str>) -> Group {
    Group {
        id: GroupId::new(),
        name: name.into(),
        defaults: ConnectionSettings {
            address: address.map(Into::into),
            ..Default::default()
        },
        location: None,
        parent_id: parent,
        tags: vec!["team".into()],
    }
}

fn host(name: &str, group_id: Option<GroupId>, key: Option<KeyRef>) -> Host {
    Host {
        forwards: Vec::new(),
        id: HostId::new(),
        name: name.into(),
        group_id,
        settings: ConnectionSettings {
            username: Some("deploy".into()),
            port: Some(2222),
            key,
            ..Default::default()
        },
        location: None,
        tags: vec!["prod".into(), "web".into()],
        os: Some(HostOs::Ubuntu),
    }
}

/// Kunden (address) > Müller > web (stored key), plus a lone host with an agent key.
fn sample() -> (Store, Vault, Group, Group) {
    let store = Store::open_in_memory().unwrap();
    let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
    let info = vault.generate_ed25519(&store, "deploy-key").unwrap();
    let kunden = group("Kunden", None, Some("kunden.example.org"));
    let mueller = group("Müller", Some(kunden.id), None);
    store.upsert_group(&kunden).unwrap();
    store.upsert_group(&mueller).unwrap();
    store
        .upsert_host(&host(
            "web",
            Some(mueller.id),
            Some(KeyRef::Stored { id: info.id }),
        ))
        .unwrap();
    let mut lone = host(
        "lone",
        None,
        Some(KeyRef::Agent {
            fingerprint: "SHA256:agent".into(),
        }),
    );
    lone.settings.address = Some("lone.example.org".into());
    store.upsert_host(&lone).unwrap();
    (store, vault, kunden, mueller)
}

#[test]
fn export_never_contains_secrets() {
    let (store, _vault, _, _) = sample();
    let record = store.list_key_records().unwrap().pop().unwrap();
    let sealed = data_encoding::BASE64.encode(&record.sealed_secret);
    let sealed_hex = data_encoding::HEXLOWER.encode(&record.sealed_secret);
    for text in [
        export_json(&store, &ExportFilter::default()).unwrap(),
        export_csv(&store, &ExportFilter::default()).unwrap(),
    ] {
        assert!(!text.contains("PRIVATE KEY"));
        assert!(!text.contains(&sealed[8..40]));
        assert!(!text.contains(&sealed_hex[8..40]));
        assert!(!text.contains("sealed"));
        assert!(text.contains("deploy-key"));
    }
    let json = export_json(&store, &ExportFilter::default()).unwrap();
    assert!(json.contains(&record.fingerprint));
    assert!(json.contains("SHA256:agent"));
}

#[test]
fn csv_round_trips_through_the_importer() {
    let (store, _vault, _, _) = sample();
    let text = export_csv(&store, &ExportFilter::default()).unwrap();
    assert!(text.starts_with(CSV_HEADER));
    let plan = crate::import::csv::parse(&text);
    assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
    let web = plan.candidates.iter().find(|c| c.name == "web").unwrap();
    assert_eq!(
        web.group_path,
        vec!["Kunden".to_owned(), "Müller".to_owned()]
    );
    assert_eq!(
        web.settings.address.as_deref(),
        Some("kunden.example.org"),
        "a flat table carries what a host inherits"
    );
    assert_eq!(web.settings.port, Some(2222));
    assert_eq!(web.tags, vec!["prod".to_owned(), "web".to_owned()]);
    assert_eq!(web.os, Some(HostOs::Ubuntu));
    assert!(plan.candidates.iter().any(|c| c.name == "lone"));
}

#[test]
fn json_keeps_the_tree() {
    let (store, _vault, kunden, mueller) = sample();
    let json: serde_json::Value =
        serde_json::from_str(&export_json(&store, &ExportFilter::default()).unwrap()).unwrap();
    assert_eq!(json["format"], "sshcraft-hosts");
    assert_eq!(json["version"], 1);
    let groups = json["groups"].as_array().unwrap();
    let m = groups.iter().find(|g| g["name"] == "Müller").unwrap();
    assert_eq!(m["parent_id"], kunden.id.0.to_string());
    let hosts = json["hosts"].as_array().unwrap();
    let web = hosts.iter().find(|h| h["name"] == "web").unwrap();
    assert_eq!(web["group_id"], mueller.id.0.to_string());
    assert_eq!(
        web["settings"]["address"],
        serde_json::Value::Null,
        "own values only"
    );
    assert_eq!(web["settings"]["key"]["name"], "deploy-key");
}

#[test]
fn filter_by_subtree() {
    let (store, _vault, _, mueller) = sample();
    let filter = ExportFilter {
        group: Some(mueller.id),
        ..ExportFilter::default()
    };
    let json: serde_json::Value =
        serde_json::from_str(&export_json(&store, &filter).unwrap()).unwrap();
    assert_eq!(json["groups"].as_array().unwrap().len(), 1);
    assert_eq!(json["hosts"].as_array().unwrap().len(), 1);
    let csv = export_csv(&store, &filter).unwrap();
    assert!(!csv.contains("lone"));
}

#[test]
fn csv_neutralizes_formulas_and_keeps_slashes() {
    let store = Store::open_in_memory().unwrap();
    let g = group("a/b", None, Some("x.org"));
    store.upsert_group(&g).unwrap();
    store
        .upsert_host(&host("=cmd()", Some(g.id), None))
        .unwrap();
    let text = export_csv(&store, &ExportFilter::default()).unwrap();
    assert!(text.contains("'=cmd()"), "{text}");
    let plan = crate::import::csv::parse(&text);
    assert_eq!(plan.candidates[0].name, "=cmd()");
    assert_eq!(plan.candidates[0].group_path, vec!["a/b".to_owned()]);
}

#[test]
fn json_subtree_keeps_inherited_values() {
    let (store, _vault, _, mueller) = sample();
    let filter = ExportFilter {
        group: Some(mueller.id),
        ..ExportFilter::default()
    };
    let json: serde_json::Value =
        serde_json::from_str(&export_json(&store, &filter).unwrap()).unwrap();
    assert_eq!(
        json["groups"][0]["defaults"]["address"],
        "kunden.example.org"
    );
}

#[test]
fn export_json_has_no_password_ids() {
    use sshcraft_model::model::{PasswordId, Proxy, ProxyEndpoint};
    let store = Store::open_in_memory().unwrap();
    let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
    let (own, proxy) = (PasswordId::new(), PasswordId::new());
    vault
        .store_password(&store, own, "hunter2-sshcraft", None)
        .unwrap();
    vault
        .store_password(&store, proxy, "hunter2-sshcraft", None)
        .unwrap();
    let mut h = host("web", None, None);
    h.settings.password = Some(own);
    h.settings.proxy = Some(Proxy::Socks5(ProxyEndpoint {
        address: "proxy.example.org".into(),
        port: 1080,
        username: Some("p".into()),
        password: Some(proxy),
    }));
    h.settings.agent_forwarding = Some(true);
    store.upsert_host(&h).unwrap();
    let json = export_json(&store, &ExportFilter::default()).unwrap();
    assert!(json.contains("\"has_password\": true"), "{json}");
    assert!(json.contains("proxy.example.org"));
    assert!(!json.contains("hunter2"));
    assert!(!json.contains(&own.0.to_string()), "{json}");
    assert!(!json.contains(&proxy.0.to_string()), "{json}");
}

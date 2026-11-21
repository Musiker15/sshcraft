//! Sharing a group, a selection or a host with another account on the same server. The other
//! account is shared with only after its safety number was compared (six groups of five
//! digits); when the server names another key for that e-mail, the window warns with the old
//! and the new number and shares nothing until the new one is accepted.

use egui::Ui;
use serde_json::{Value, json};

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::theme::Tokens;
use crate::widgets::visible;

/// The share dialog.
#[derive(Debug, Clone)]
pub struct ShareForm {
    server: String,
    what: Value,
    title: String,
    pub email: String,
    /// What `share.find_recipient` found.
    pub recipient: Option<Value>,
    pub ticked: bool,
    pub write: bool,
    pub with_keys: bool,
}

/// Settings > Sync > Shares.
#[derive(Debug, Clone, Default)]
pub struct SharesView {
    /// Every share of every server, each with its `server`.
    pub list: Option<Vec<Value>>,
    /// A recipient waiting for the removal to be confirmed: server, area, account, keys went along.
    removing: Option<(String, String, Value, bool)>,
}

impl SharesView {
    /// Whether the removal of a recipient waits for its confirmation.
    pub fn removing_open(&self) -> bool {
        self.removing.is_some()
    }
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn server_of(app: &SshcraftApp, group: &str, host: Option<&str>) -> Option<String> {
    let place = match host {
        Some(h) => app.data.hosts.iter().find(|r| r.host.id.0.to_string() == h).and_then(|r| r.host.location),
        None => app.data.groups.iter().find(|r| r.group.id.0.to_string() == group).and_then(|r| r.group.location),
    };
    place.map(|s| s.0.to_string())
}

/// Opens the share dialog for group `group`, or for one of its hosts.
pub fn ask_share(app: &mut SshcraftApp, group: &str, host: Option<&str>) {
    let Some(server) = server_of(app, group, host) else {
        app.toasts.error(tr("Move it to a sync server first: shares live there."));
        return;
    };
    let (what, title) = match host {
        Some(h) => (
            json!({ "kind": "host", "host": h }),
            app.data.hosts.iter().find(|r| r.host.id.0.to_string() == h).map(|r| r.host.name.clone()).unwrap_or_default(),
        ),
        None => (
            json!({ "kind": "group_all", "group": group }),
            app.data.groups.iter().find(|r| r.group.id.0.to_string() == group).map(|r| r.group.name.clone()).unwrap_or_default(),
        ),
    };
    app.ui.share = Some(ShareForm { server, what, title, email: String::new(), recipient: None, ticked: false, write: false, with_keys: false });
}

fn find(app: &mut SshcraftApp, form: &ShareForm) {
    let (server, email) = (form.server.clone(), form.email.trim().to_string());
    app.runner.run(
        "share.find_recipient",
        json!({ "server": server, "email": email }),
        Box::new(move |app, result| match result {
            Ok(found) => {
                if let Some(form) = app.ui.share.as_mut().filter(|f| f.email.trim() == email) {
                    form.recipient = Some(found);
                    form.ticked = false;
                }
            }
            Err(e) => app.toasts.error(&e.to_string()),
        }),
    );
}

fn share(app: &mut SshcraftApp, form: &ShareForm, recipient: &Value) {
    let Some(generation) = app.forms.start() else { return };
    let account = recipient.get("account").cloned().unwrap_or_default();
    let confirmed = recipient.pointer("/known/state").and_then(Value::as_str) == Some("confirmed");
    let session = app.session.clone();
    let (server, email, what) = (form.server.clone(), text(recipient, "email"), form.what.clone());
    let grants = json!([{ "account": account, "role": if form.write { "write" } else { "read" } }]);
    let with_keys = form.with_keys;
    app.runner.run_future(
        async move {
            // The user compared the number: that trust is kept first, then the share is made.
            if !confirmed {
                session
                    .execute(
                        sshcraft_engine::Caller::User,
                        "share.confirm_recipient",
                        json!({ "server": server, "email": email, "account": account }),
                    )
                    .await?;
            }
            session
                .execute(
                    sshcraft_engine::Caller::User,
                    "share.items",
                    json!({ "server": server, "what": what, "grants": grants, "with_keys": with_keys }),
                )
                .await
        },
        Box::new(move |app, result| {
            let current = app.forms.done(generation);
            match result {
                Ok(_) => {
                    if current {
                        app.ui.share = None;
                    }
                    app.toasts.info(tr("Shared."));
                    app.data.dirty |= crate::data::Refresh::DATA;
                }
                Err(e) => app.toasts.error(&e.to_string()),
            }
        }),
    );
}

pub fn dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut form) = app.ui.share.take() else { return };
    let tokens = Tokens::get(ctx);
    let busy = app.forms.busy();
    let (mut keep, mut do_find, mut do_share) = (true, false, false);
    let modal = crate::widgets::modal(ctx, egui::Id::unique("sshcraft-share")).show(ctx, |ui| {
        crate::widgets::fit_dialog(ui);
        ui.set_width(crate::widgets::fit(ui, 540.0));
        crate::widgets::buttons::title(ui, trf("Share {name}", &[("name", &visible(&form.title))]));
        ui.label(egui::RichText::new(tr("Only with accounts on the same server.")).color(tokens.text_dim));
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(tr("Email"));
            ui.add(egui::TextEdit::singleline(&mut form.email).desired_width(crate::widgets::fit(ui, 260.0)).char_limit(320)).labelled_by(label.id);
            do_find = ui.add_enabled(!form.email.trim().is_empty(), egui::Button::new(tr("Find"))).clicked();
        });
        if let Some(r) = form.recipient.clone() {
            let name = visible(&text(&r, "name"));
            let number = text(&r, "safety_number");
            let state = r.pointer("/known/state").and_then(Value::as_str).unwrap_or("new");
            ui.add_space(6.0);
            ui.label(trf("{name} ({email})", &[("name", &name), ("email", &visible(&text(&r, "email")))]));
            if state == "changed" {
                let old = r.pointer("/known/previous_safety_number").and_then(Value::as_str).unwrap_or_default();
                ui.label(
                    egui::RichText::new(trf(
                        "The key of this account changed. Before: {old}. Now: {new}. Ask {name} whether they set up their account again.",
                        &[("old", old), ("new", &number), ("name", &name)],
                    ))
                    .color(tokens.danger),
                );
                ui.checkbox(&mut form.ticked, trf("I compared the new safety number with {name}", &[("name", &name)]));
            } else {
                ui.label(trf("Their safety number: {number}", &[("number", &number)]));
                if state != "confirmed" {
                    ui.label(
                        egui::RichText::new(tr("Compare it with them by phone or in person; it must be the same on their device."))
                            .color(tokens.text_dim),
                    );
                    ui.checkbox(&mut form.ticked, trf("I compared the safety number with {name}", &[("name", &name)]));
                }
            }
            ui.horizontal_wrapped(|ui| {
                ui.radio_value(&mut form.write, false, tr("Read"));
                ui.radio_value(&mut form.write, true, tr("Read and change"));
            });
            ui.checkbox(&mut form.with_keys, tr("Share the keys too (never keys in the SSH agent)"));
            let ready = form.ticked || state == "confirmed";
            ui.horizontal_wrapped(|ui| {
                do_share = ui.add_enabled(ready && !busy, egui::Button::new(tr("Share"))).clicked();
                if ui.button(tr("Cancel")).clicked() {
                    keep = false;
                }
            });
        } else if ui.button(tr("Cancel")).clicked() {
            keep = false;
        }
    });
    if modal.should_close() {
        keep = false;
    }
    if do_find {
        find(app, &form);
    }
    if do_share && let Some(r) = form.recipient.clone() {
        share(app, &form, &r);
    }
    if keep {
        app.ui.share = Some(form);
    }
}

/// Fetches the shares of every server.
pub fn load(app: &mut SshcraftApp) {
    let session = app.session.clone();
    app.runner.run_future(
        async move {
            // The servers as the engine has them: the window's list may be reloading.
            let rows = session.execute(sshcraft_engine::Caller::User, "sync.servers", json!({})).await?;
            let servers: Vec<String> = rows.as_array().into_iter().flatten().map(|s| text(s, "id")).filter(|id| !id.is_empty()).collect();
            let mut all = Vec::new();
            for server in servers {
                let list = session.execute(sshcraft_engine::Caller::User, "share.list", json!({ "server": server })).await?;
                for mut share in list.as_array().cloned().unwrap_or_default() {
                    if let Some(map) = share.as_object_mut() {
                        map.insert("server".into(), json!(server));
                    }
                    all.push(share);
                }
            }
            Ok(Value::Array(all))
        },
        Box::new(|app, result| match result {
            Ok(Value::Array(list)) => app.ui.shares.list = Some(list),
            Ok(_) => {}
            Err(e) => app.toasts.error(&e.to_string()),
        }),
    );
}

/// Own and received shares, under Settings > Sync.
pub fn list(app: &mut SshcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    if ui.button(tr("Shares")).clicked() {
        load(app);
    }
    let Some(shares) = app.ui.shares.list.clone() else { return };
    if shares.is_empty() {
        ui.label(egui::RichText::new(tr("Nothing shared.")).color(tokens.text_dim));
    }
    let mut end = None;
    for share in &shares {
        let (server, area) = (text(share, "server"), text(share, "area"));
        let owned = share.get("owned") == Some(&Value::Bool(true));
        let group = share
            .get("group")
            .and_then(Value::as_str)
            .and_then(|g| app.data.groups.iter().find(|r| r.group.id.0.to_string() == g))
            .map(|r| r.group.name.clone());
        let what = group
            .unwrap_or_else(|| trf("{count} hosts", &[("count", &share.get("hosts").and_then(Value::as_array).map_or(0, Vec::len).to_string())]));
        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(visible(&what)).strong());
                ui.label(if owned { tr("shared by you") } else { tr("shared with you") });
                if owned && ui.button(tr("End share")).clicked() {
                    end = Some((server.clone(), area.clone()));
                }
            });
            for grant in share.get("grants").and_then(Value::as_array).cloned().unwrap_or_default() {
                ui.horizontal_wrapped(|ui| {
                    let who = grant.get("email").and_then(Value::as_str).map_or_else(|| tr("Unknown account").to_string(), visible);
                    let role = if text(&grant, "role") == "write" { tr("read and change") } else { tr("read") };
                    ui.label(format!("{who} ({role})"));
                    if owned && ui.button(tr("Remove recipient")).clicked() {
                        let keys = share.get("with_keys") == Some(&Value::Bool(true));
                        app.ui.shares.removing = Some((server.clone(), area.clone(), grant.get("account").cloned().unwrap_or_default(), keys));
                    }
                });
            }
        });
    }
    if let Some((server, area)) = end {
        crate::confirm::ask(
            app,
            tr("End share").to_string(),
            tr("Every recipient loses access to these hosts at once."),
            tr("End sharing"),
            crate::confirm::Pending::EndShare { server, area },
        );
    }
}

/// Ends the share of `area` on `server` (after the user confirmed it).
pub fn end_share(app: &mut SshcraftApp, server: String, area: String) {
    {
        app.runner.run(
            "share.end",
            json!({ "server": server, "area": area }),
            Box::new(|app, result| {
                if let Err(e) = result {
                    app.toasts.error(&e.to_string());
                }
                app.data.dirty |= crate::data::Refresh::DATA;
                load(app);
            }),
        );
    }
}

/// Confirms removing a recipient: their access ends and the share's key changes.
pub fn remove_dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some((server, area, account, keys)) = app.ui.shares.removing.clone() else { return };
    let tokens = Tokens::get(ctx);
    let (mut close, mut go) = (false, false);
    crate::widgets::modal(ctx, egui::Id::unique("sshcraft-remove-recipient")).show(ctx, |ui| {
        crate::widgets::fit_dialog(ui);
        ui.set_width(crate::widgets::fit(ui, 460.0));
        crate::widgets::buttons::title(ui, tr("Remove recipient"));
        ui.label(tr("They lose access, and the share gets a new key."));
        if keys {
            ui.label(
                egui::RichText::new(tr("Keys went along with this share: they may have been copied. Change them on the servers."))
                    .color(tokens.warning),
            );
        }
        ui.horizontal_wrapped(|ui| {
            go = ui.button(tr("Remove")).clicked();
            close = ui.button(tr("Cancel")).clicked();
        });
    });
    if close {
        app.ui.shares.removing = None;
    }
    if go {
        app.ui.shares.removing = None;
        let session = app.session.clone();
        app.runner.run_future(
            async move {
                // The recipients as they are now, not as the list showed them: one added on
                // another device since stays.
                let list = session.execute(sshcraft_engine::Caller::User, "share.list", json!({ "server": server })).await?;
                let remaining: Vec<Value> = list
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|s| text(s, "area") == area)
                    .and_then(|s| s.get("grants").and_then(Value::as_array).cloned())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|g| g.get("account") != Some(&account))
                    .map(|g| json!({ "account": g.get("account"), "role": g.get("role") }))
                    .collect();
                session
                    .execute(sshcraft_engine::Caller::User, "share.set_grants", json!({ "server": server, "area": area, "grants": remaining }))
                    .await
            },
            Box::new(|app, result| {
                if let Err(e) = result {
                    app.toasts.error(&e.to_string());
                }
                load(app);
            }),
        );
    }
}

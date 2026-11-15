//! The menus: one definition ([`menu_bar`]) for the native macOS menu bar and the bar in the
//! window on Windows and Linux, laid out for each platform by craft-menu.
//!
//! Items run the interface's commands (`ui.*`, [`crate::commands`]) by id, with their current
//! shortcuts, so a menu click and its shortcut run the same command. The engine's commands carry
//! no menu path: what changes data opens its form from a page. Nothing runs from a menu while a
//! dialog, a question or the palette holds the window ([`gate`]).

use craft_menu::{CommandRef, Gate, Item, ItemRole, ItemState, LayoutOptions, Menu, MenuBar, MenuRole, Node, Target};

use crate::app::SshcraftApp;
use crate::i18n::tr;
use crate::state::Page;

/// The app's name in the items the layouts write (About, Hide, Quit).
pub const APP_NAME: &str = "SSHCraft";

/// A separator in the tables below.
const SEP: (&str, &str) = ("", "");

const FILE: &[(&str, &str)] = &[("ui.tab.new", "New tab"), ("ui.tab.close", "Close tab"), SEP, ("ui.app.quit", "Quit")];
const EDIT: &[(&str, &str)] = &[("ui.terminal.copy", "Copy"), ("ui.terminal.paste", "Paste"), SEP, ("ui.page.settings", "Settings")];
const VIEW: &[(&str, &str)] = &[
    ("ui.palette", "Command palette"),
    SEP,
    ("ui.page.hosts", "Hosts"),
    ("ui.page.keys", "Keys"),
    ("ui.page.snippets", "Snippets"),
    ("ui.page.forwarding", "Port forwarding"),
    ("ui.page.logs", "Logs"),
    SEP,
    ("ui.terminal.sidebar", "Snippets and history beside the terminal"),
];
const SESSION: &[(&str, &str)] = &[
    ("ui.pane.split", "Split: add a pane"),
    ("ui.pane.close", "Close pane"),
    ("ui.pane.next", "Next pane"),
    ("ui.pane.previous", "Previous pane"),
    ("ui.pane.layout", "Switch between grid and one large pane"),
    SEP,
    ("ui.pane.broadcast", "Broadcast"),
    ("ui.terminal.bookmark", "Bookmark in the session log"),
];
const WINDOW: &[(&str, &str)] = &[("ui.tab.next", "Next tab"), ("ui.tab.previous", "Previous tab")];
const HELP: &[(&str, &str)] = &[("ui.help.about", "About SSHCraft")];

/// The labels the layouts and the bar write themselves, through the app's translations.
fn translate(english: &'static str) -> String {
    // The bar's menu of titles that don't fit is "⋯" (U+22EF), a sign no bundled font has; the
    // ordinary ellipsis is in every one.
    if english == "\u{22ef}" {
        return "…".to_owned();
    }
    tr(english).to_owned()
}

/// The layouts' options: the app's name and its translations.
pub fn layout_options() -> LayoutOptions<'static> {
    LayoutOptions { label: &translate, ..LayoutOptions::new(APP_NAME) }
}

fn items(app: &SshcraftApp, rows: &[(&'static str, &'static str)]) -> Vec<Node> {
    rows.iter()
        .map(|&(id, label)| {
            if id.is_empty() {
                return Node::Separator;
            }
            let mut item = Item::new(id, tr(label));
            if let Some(chord) = app.bindings.chords(id).first() {
                item = item.shortcut(*chord);
            }
            if id == "ui.pane.broadcast" {
                item = item.check(false);
            }
            item.into()
        })
        .collect()
}

/// The app's menus as they are now, before the platform layout: labels, shortcuts, enabled and
/// checked. A dialog greys nothing here; that is the [`gate`]'s job.
pub fn menu_bar(app: &SshcraftApp) -> MenuBar {
    let menu = |title: &'static str, role: MenuRole, rows| Menu::new(tr(title), items(app, rows)).role(role);
    let mut bar = MenuBar {
        menus: vec![
            menu("File", MenuRole::File, FILE),
            menu("Edit", MenuRole::Edit, EDIT),
            menu("View", MenuRole::View, VIEW),
            menu("Session", MenuRole::Other, SESSION),
            menu("Window", MenuRole::Window, WINDOW),
            menu("Help", MenuRole::Help, HELP),
        ],
        documents: Vec::new(),
    };
    bar.tag_item(&CommandRef::new("ui.app.quit"), ItemRole::Quit);
    bar.tag_item(&CommandRef::new("ui.page.settings"), ItemRole::Settings);
    bar.tag_item(&CommandRef::new("ui.help.about"), ItemRole::About);
    craft_menu::apply_state(&mut bar, |command| state(app, command));
    bar
}

/// Whether an item can run now, and the broadcast's check mark.
fn state(app: &SshcraftApp, command: &CommandRef) -> ItemState {
    let enabled = crate::commands::find(&command.id).is_none_or(|c| (c.enabled)(app));
    let broadcast = || matches!(app.ui.page, Page::Tab(i) if app.ui.tabs.get(i).is_some_and(|t| t.panes.broadcast));
    let checked = (command.id == "ui.pane.broadcast").then(broadcast);
    ItemState { enabled, checked }
}

/// What may run from the menus now: nothing while a dialog, a question, the quit question or the
/// palette holds the window, or while a shortcut is recorded.
pub fn gate(app: &SshcraftApp, ctx: &egui::Context) -> Gate {
    let dialog = ctx.memory(|m| m.top_modal_layer().is_some());
    Gate {
        modal: dialog || app.ui.palette.open || app.ui.quit.asking || !app.prompts.open.is_empty(),
        recording_shortcut: app.ui.settings.recording.is_some(),
        text_editing: ctx.text_edit_focused(),
    }
}

/// Runs a command chosen in a menu, as its shortcut would.
pub fn run(app: &mut SshcraftApp, command: &CommandRef) {
    if crate::commands::find(&command.id).is_some() {
        crate::commands::run(app, &command.id);
    }
}

/// The bar in the window (Windows, Linux, and macOS when the native one couldn't be installed).
/// Runs the commands chosen. `max_width` leaves room for what follows it in the row.
pub fn bar_ui(app: &mut SshcraftApp, ui: &mut egui::Ui, max_width: f32) {
    let ctx = ui.ctx().clone();
    let bar = craft_menu::layout(&menu_bar(app), Target::current(), &layout_options()).bar;
    // While a terminal has the keyboard, the bar sees none of it: Alt+F is the shell's (forward a
    // word), F10 the program's, and a tap on Alt must not put the arrow keys on the bar. The mouse
    // still opens the menus, and an open menu has the keys (the terminal then gets none). The keys
    // are given back unchanged for the terminal after the bar.
    let held = (app.in_terminal() && !ctx.text_edit_focused() && !craft_menu::bar::is_open(&ctx)).then(|| {
        ctx.input_mut(|i| {
            let kept = (i.events.clone(), i.raw.events.clone());
            i.events.retain(|e| !keyboard(e));
            i.raw.events.retain(|e| !keyboard(e));
            kept
        })
    });
    let chosen = craft_menu::MenuBarUi::new(&bar)
        .gate(gate(app, &ctx))
        .label(&translate)
        .id(egui::Id::unique("sshcraft-menu-bar"))
        .shrink_to_fit(max_width)
        .show(ui);
    if let Some((events, raw)) = held {
        ctx.input_mut(|i| {
            i.events = events;
            i.raw.events = raw;
        });
    }
    for command in chosen {
        run(app, &command);
    }
}

/// What a terminal reads from the keyboard: keys, typed text and the clipboard keys.
fn keyboard(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_) | egui::Event::Ime(_)
    )
}

/// The native menu bar (macOS), once, from eframe's app creator. `false` when AppKit refused it:
/// the bar in the window stays.
#[cfg(target_os = "macos")]
pub fn install_native(app: &mut SshcraftApp, ctx: &egui::Context) -> bool {
    let bar = craft_menu::mac_layout(&menu_bar(app), &layout_options()).bar;
    app.native_menu = craft_menu::mac::install_with(ctx, &bar, craft_menu::mac::MacOptions::default().window_list(false));
    app.native_menu.is_some()
}

/// Whether the menus are the system's (macOS with the native bar installed).
pub fn native(app: &SshcraftApp) -> bool {
    #[cfg(target_os = "macos")]
    {
        app.native_menu.is_some()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        false
    }
}

/// The native menu's clicks since the last frame, run like those in the window; then the menu
/// follows the app's state.
#[cfg(target_os = "macos")]
pub fn native_frame(app: &mut SshcraftApp, ctx: &egui::Context) {
    let gate = gate(app, ctx);
    let clicks = match app.native_menu.as_mut() {
        Some(native) => {
            native.set_gate(gate);
            native.take_clicks(ctx)
        }
        None => return,
    };
    for command in clicks {
        run(app, &command);
    }
    let layout = craft_menu::mac_layout(&menu_bar(app), &layout_options());
    let gate = self::gate(app, ctx);
    if let Some(native) = app.native_menu.as_mut() {
        native.set_gate(gate);
        let _clashes = native.sync(layout);
    }
}

/// Help › About: the app's name, version and licence.
pub fn about(app: &mut SshcraftApp, ctx: &egui::Context) {
    if !app.ui.about {
        return;
    }
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-about")).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.heading(APP_NAME);
        ui.label(crate::i18n::trf("Version {version}", &[("version", env!("CARGO_PKG_VERSION"))]));
        ui.add_space(6.0);
        ui.label(tr("An SSH client with end-to-end encrypted, self-hosted sync."));
        ui.label(crate::i18n::trf("Licence: {licence}", &[("licence", env!("CARGO_PKG_LICENSE"))]));
        ui.add_space(10.0);
        if ui.button(tr("Close")).clicked() {
            close = true;
        }
    });
    if close || modal.should_close() {
        app.ui.about = false;
    }
}

/// The native menu's key equivalents, fed to egui as the keys that were pressed; call it from
/// `eframe::App::raw_input_hook`, before egui reads the input. Does nothing off macOS.
pub fn native_raw_input(app: &mut SshcraftApp, raw: &mut egui::RawInput) {
    #[cfg(target_os = "macos")]
    if let Some(native) = app.native_menu.as_mut() {
        native.raw_input(raw);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, raw);
}

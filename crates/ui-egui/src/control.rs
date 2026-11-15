//! The window's side of the control channel (`--control`, a developer tool): requests wait in a
//! queue and are answered in the window's frame. Pointer and key input goes into the next frames
//! through eframe's raw-input hook ([`SshcraftApp::take_control_input`]), so it reaches the
//! widgets exactly as a person's would. Every parameter is checked before anything happens.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use serde_json::{Map, Value, json};
use tokio::sync::oneshot;

use crate::app::SshcraftApp;
use crate::state::{Page, PaneSlot};

/// Coordinates beyond this are refused.
const MAX_COORD: f64 = 1_000_000.0;
/// The longest text `ui.text` types at once.
const MAX_TEXT: usize = 4096;
/// Requests waiting for the window at most; more are refused at once.
pub const MAX_QUEUED: usize = 64;
/// The most clicks one `ui.click` makes.
const MAX_COUNT: u64 = 8;

/// One request and where its answer goes.
pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    pub reply: oneshot::Sender<Result<Value, String>>,
}

/// Requests from the control channel to the window; pushing one wakes the window.
#[derive(Clone)]
pub struct ControlQueue {
    items: Arc<Mutex<VecDeque<ControlRequest>>>,
    wake: egui::Context,
}

impl ControlQueue {
    pub fn new(wake: egui::Context) -> Self {
        Self { items: Arc::default(), wake }
    }

    /// Queues `request`; with [`MAX_QUEUED`] waiting already, it is refused at once.
    pub fn push(&self, request: ControlRequest) {
        let mut items = self.items.lock().unwrap_or_else(PoisonError::into_inner);
        if items.len() >= MAX_QUEUED {
            drop(items);
            let _ = request.reply.send(Err("too many requests waiting for the window".into()));
            return;
        }
        items.push_back(request);
        drop(items);
        self.wake.request_repaint();
    }

    fn take(&self) -> Vec<ControlRequest> {
        self.items.lock().unwrap_or_else(PoisonError::into_inner).drain(..).collect()
    }
}

type Reply = oneshot::Sender<Result<Value, String>>;

/// The control channel's state in the window.
pub struct Control {
    pub queue: ControlQueue,
    /// Input for the coming frames, one list per frame, each with the request it belongs to.
    frames: VecDeque<(u64, Vec<Event>)>,
    /// Input requests whose frames have not all gone in yet.
    waiting: Vec<(u64, Reply)>,
    /// Input requests whose last frame went in at take number `.1`; answered once a later frame
    /// began, so the input has taken effect.
    ready: Vec<(Reply, u64)>,
    next: u64,
    /// Frames begun (calls of the raw-input hook).
    takes: u64,
}

impl Control {
    pub fn new(queue: ControlQueue) -> Self {
        Self { queue, frames: VecDeque::new(), waiting: Vec::new(), ready: Vec::new(), next: 1, takes: 0 }
    }

    /// Whether request `owner` still has someone waiting for it.
    fn wanted(&self, owner: u64) -> bool {
        self.waiting.iter().any(|(o, reply)| *o == owner && !reply.is_closed())
    }
}

fn object<'a>(params: &'a Value, allowed: &[&str]) -> Result<&'a Map<String, Value>, String> {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    let map = match params {
        Value::Null => EMPTY.get_or_init(Map::new),
        Value::Object(map) => map,
        _ => return Err("params must be an object".into()),
    };
    if let Some(key) = map.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(format!("unknown parameter `{}`", key.chars().filter(|c| !c.is_control()).take(40).collect::<String>()));
    }
    Ok(map)
}

fn coord(map: &Map<String, Value>, name: &str) -> Result<f32, String> {
    let value = map.get(name).and_then(Value::as_f64).ok_or_else(|| format!("`{name}` must be a number"))?;
    if !value.is_finite() || value.abs() > MAX_COORD {
        return Err(format!("`{name}` is out of range"));
    }
    #[allow(clippy::cast_possible_truncation)] // within ±1 000 000, checked above
    Ok(value as f32)
}

fn flag(map: &Map<String, Value>, name: &str) -> Result<bool, String> {
    match map.get(name) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("`{name}` must be true or false")),
    }
}

fn point(value: Option<&Value>, name: &str) -> Result<Pos2, String> {
    let pair = value.and_then(Value::as_array).filter(|a| a.len() == 2).ok_or_else(|| format!("`{name}` must be [x, y]"))?;
    let mut map = Map::new();
    map.insert("x".into(), pair.first().cloned().unwrap_or_default());
    map.insert("y".into(), pair.get(1).cloned().unwrap_or_default());
    Ok(Pos2::new(coord(&map, "x")?, coord(&map, "y")?))
}

fn page_name(page: Page) -> String {
    match page {
        Page::Hosts => "hosts".into(),
        Page::Keys => "keys".into(),
        Page::Snippets => "snippets".into(),
        Page::Forwarding => "forwarding".into(),
        Page::Logs => "logs".into(),
        Page::Settings => "settings".into(),
        Page::Tab(i) => format!("tab:{i}"),
    }
}

fn parse_page(text: &str, tabs: usize) -> Option<Page> {
    Some(match text {
        "hosts" => Page::Hosts,
        "keys" => Page::Keys,
        "snippets" => Page::Snippets,
        "forwarding" => Page::Forwarding,
        "logs" => Page::Logs,
        "settings" => Page::Settings,
        other => {
            let index: usize = other.strip_prefix("tab:")?.parse().ok()?;
            (index < tabs).then_some(Page::Tab(index))?
        }
    })
}

/// What the window shows: for scripts and tests.
fn inspect(app: &SshcraftApp, ctx: &egui::Context) -> Value {
    let rect = ctx.content_rect();
    let tabs: Vec<Value> = app
        .ui
        .tabs
        .iter()
        .map(|t| {
            let panes: Vec<Value> = t
                .panes
                .panes
                .iter()
                .map(|slot| match slot {
                    PaneSlot::New { .. } => json!({ "kind": "new" }),
                    PaneSlot::Session { session } => {
                        let title = app.terminals.get(session).map(|s| s.title.clone()).unwrap_or_default();
                        json!({ "kind": "session", "session": session, "title": title })
                    }
                })
                .collect();
            json!({ "title": t.title, "panes": panes, "focused": t.panes.focused, "broadcast": t.panes.broadcast })
        })
        .collect();
    let prompts: Vec<Value> = app
        .prompts
        .open
        .iter()
        .map(|p| {
            let kind = match p.kind {
                crate::prompts::PromptKind::HostKey { .. } => "host_key",
                crate::prompts::PromptKind::Auth { .. } => "sign_in",
                crate::prompts::PromptKind::Agent { .. } => "agent",
            };
            json!({ "id": p.id, "kind": kind })
        })
        .collect();
    json!({
        "page": page_name(app.ui.page),
        "settingsSection": format!("{:?}", app.ui.settings.section),
        "tabs": tabs,
        "prompts": prompts,
        "notices": app.toasts.texts(),
        "window": { "width": rect.width(), "height": rect.height() },
        "dialogOpen": crate::forms::shape(&app.ui) != crate::forms::shape(&crate::state::UiState::default()),
    })
}

/// The text of session `id`'s screen, or of the focused pane of the shown tab.
fn terminal_text(app: &SshcraftApp, params: &Value) -> Result<Value, String> {
    let map = object(params, &["session"])?;
    let session = match map.get("session") {
        Some(Value::String(text)) => serde_json::from_value(Value::String(text.clone())).map_err(|_| "`session` is not a session id".to_string())?,
        Some(_) => return Err("`session` must be a session id".into()),
        None => {
            let Page::Tab(i) = app.ui.page else { return Err("no terminal is shown".into()) };
            match app.ui.tabs.get(i).and_then(|t| t.panes.focused_slot()) {
                Some(PaneSlot::Session { session }) => session,
                _ => return Err("no terminal is shown".into()),
            }
        }
    };
    let tab = app.terminals.get(&session).ok_or("no such session in this window")?;
    Ok(json!({ "text": tab.term.visible_text() }))
}

fn png_base64(image: &egui::ColorImage) -> Result<Value, String> {
    let [width, height] = image.size;
    let w = u32::try_from(width).map_err(|_| "too wide")?;
    let h = u32::try_from(height).map_err(|_| "too high")?;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    let png = png_of(w, h, bytes)?;
    Ok(json!({ "width": width, "height": height, "png_base64": data_encoding::BASE64.encode(&png) }))
}

/// `rgba` (`width` × `height`, 4 bytes a pixel) as PNG.
pub fn png_of(width: u32, height: u32, rgba: Vec<u8>) -> Result<Vec<u8>, String> {
    let image = image::RgbaImage::from_raw(width, height, rgba).ok_or("the picture's size does not fit its pixels")?;
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// Input events for one key press and release.
fn key_frames(map: &Map<String, Value>) -> Result<Vec<Vec<Event>>, String> {
    let name = map.get("key").and_then(Value::as_str).ok_or("`key` must be a key name")?;
    let key = Key::from_name(name).ok_or_else(|| format!("unknown key `{}`", name.chars().take(32).collect::<String>()))?;
    let modifiers = Modifiers {
        alt: flag(map, "alt")?,
        ctrl: flag(map, "ctrl")?,
        shift: flag(map, "shift")?,
        mac_cmd: false,
        command: flag(map, "command")? || flag(map, "ctrl")?,
    };
    let event = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
    Ok(vec![vec![event(true)], vec![event(false)]])
}

fn button_of(map: &Map<String, Value>) -> Result<PointerButton, String> {
    match map.get("button").and_then(Value::as_str) {
        None | Some("left") => Ok(PointerButton::Primary),
        Some("right" | "secondary") => Ok(PointerButton::Secondary),
        Some("middle") => Ok(PointerButton::Middle),
        Some(_) => Err("`button` is left, right or middle".into()),
    }
}

fn click_frames(map: &Map<String, Value>) -> Result<Vec<Vec<Event>>, String> {
    let at = Pos2::new(coord(map, "x")?, coord(map, "y")?);
    let button = button_of(map)?;
    let count = match map.get("count") {
        None | Some(Value::Null) => 1,
        Some(v) => v.as_u64().filter(|c| (1..=MAX_COUNT).contains(c)).ok_or("`count` is 1 to 8")?,
    };
    let press = |pressed| Event::PointerButton { pos: at, button, pressed, modifiers: Modifiers::NONE };
    let mut frames = vec![vec![Event::PointerMoved(at)]];
    for _ in 0..count {
        frames.push(vec![press(true)]);
        frames.push(vec![press(false)]);
    }
    Ok(frames)
}

fn drag_frames(map: &Map<String, Value>) -> Result<Vec<Vec<Event>>, String> {
    let from = point(map.get("from"), "from")?;
    let to = point(map.get("to"), "to")?;
    let press = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    let mut frames = vec![vec![Event::PointerMoved(from)], vec![press(from, true)]];
    for step in 1..=6 {
        frames.push(vec![Event::PointerMoved(from + (to - from) * (step as f32 / 6.0))]);
    }
    frames.push(vec![press(to, false)]);
    Ok(frames)
}

/// The input frames for an input method, or `None` for the others.
fn input_frames(method: &str, params: &Value) -> Option<Result<Vec<Vec<Event>>, String>> {
    Some(match method {
        "ui.click" => object(params, &["x", "y", "button", "count"]).and_then(click_frames),
        "ui.move" => object(params, &["x", "y"]).and_then(|m| Ok(vec![vec![Event::PointerMoved(Pos2::new(coord(m, "x")?, coord(m, "y")?))]])),
        "ui.drag" => object(params, &["from", "to"]).and_then(drag_frames),
        "ui.key" => object(params, &["key", "shift", "ctrl", "alt", "command"]).and_then(key_frames),
        "ui.text" => object(params, &["text"]).and_then(|m| {
            let text = m.get("text").and_then(Value::as_str).ok_or("`text` must be text")?;
            if text.chars().count() > MAX_TEXT {
                return Err("`text` is longer than 4096 characters".into());
            }
            Ok(vec![vec![Event::Text(text.to_string())]])
        }),
        _ => return None,
    })
}

/// Answers the queued requests; called every frame while the control channel is on.
pub fn process(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(control) = app.control.as_mut() else { return };
    // Input that took effect in a frame that has ended: its answer goes now.
    let takes = control.takes;
    let (done, later): (Vec<_>, Vec<_>) = std::mem::take(&mut control.ready).into_iter().partition(|(_, at)| *at < takes);
    control.ready = later;
    for (reply, _) in done {
        let _ = reply.send(Ok(Value::Null));
    }
    if !control.frames.is_empty() || !control.waiting.is_empty() || !control.ready.is_empty() {
        // Input waits for the next frames: they must come.
        ctx.request_repaint();
    }
    let requests = control.queue.take();
    for ControlRequest { method, params, reply } in requests {
        // Nobody waits for it any more (timed out, cancelled, gone): it does nothing.
        if reply.is_closed() {
            continue;
        }
        if let Some(frames) = input_frames(&method, &params) {
            match frames {
                Ok(frames) => {
                    let Some(control) = app.control.as_mut() else { return };
                    let owner = control.next;
                    control.next = control.next.wrapping_add(1);
                    control.frames.extend(frames.into_iter().map(|f| (owner, f)));
                    control.waiting.push((owner, reply));
                    ctx.request_repaint();
                }
                Err(e) => {
                    let _ = reply.send(Err(e));
                }
            }
            continue;
        }
        let answer = match method.as_str() {
            "window.list" => object(&params, &[]).map(|_| json!([{ "id": "main", "title": "SSHCraft" }])),
            "ui.inspect" => object(&params, &[]).map(|_| inspect(app, ctx)),
            "ui.set" => object(&params, &["page"]).and_then(|m| {
                let name = m.get("page").and_then(Value::as_str).ok_or("`page` must be a page name")?;
                let page = parse_page(name, app.ui.tabs.len()).ok_or("unknown page")?;
                app.ui.page = page;
                Ok(Value::Null)
            }),
            "ui.resize" => object(&params, &["width", "height"]).and_then(|m| {
                let (w, h) = (coord(m, "width")?, coord(m, "height")?);
                if w < 1.0 || h < 1.0 {
                    return Err("the size must be positive".into());
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
                Ok(Value::Null)
            }),
            "ui.focus" => object(&params, &[]).map(|_| {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                Value::Null
            }),
            "ui.screenshot" => match object(&params, &[]) {
                Ok(_) => {
                    let callback = egui::ScreenshotCallback::new(move |image| {
                        let _ = reply.send(png_base64(&image));
                    });
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(callback));
                    ctx.request_repaint();
                    continue;
                }
                Err(e) => Err(e),
            },
            "terminal.read" => terminal_text(app, &params),
            "app.quit" => object(&params, &[]).map(|_| {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                Value::Null
            }),
            other => Err(format!("unknown method {}", other.chars().filter(|c| !c.is_control()).take(64).collect::<String>())),
        };
        let _ = reply.send(answer);
    }
}

impl SshcraftApp {
    /// Turns the control channel on; requests pushed to the returned queue are answered in the
    /// window's frames.
    pub fn enable_control(&mut self, ctx: &egui::Context) -> ControlQueue {
        let queue = ControlQueue::new(ctx.clone());
        self.control = Some(Control::new(queue.clone()));
        queue
    }

    /// The input for the next frame (eframe's raw-input hook puts it in front of the frame).
    pub fn take_control_input(&mut self) -> Vec<Event> {
        let Some(control) = self.control.as_mut() else { return Vec::new() };
        control.takes = control.takes.wrapping_add(1);
        // The input of a request nobody waits for any more is dropped, also halfway through.
        while control.frames.front().is_some_and(|(owner, _)| !control.wanted(*owner)) {
            control.frames.pop_front();
        }
        control.waiting.retain(|(_, reply)| !reply.is_closed());
        let Some((owner, events)) = control.frames.pop_front() else { return Vec::new() };
        if !control.frames.iter().any(|(o, _)| *o == owner)
            && let Some(at) = control.waiting.iter().position(|(o, _)| *o == owner)
        {
            let (_, reply) = control.waiting.swap_remove(at);
            control.ready.push((reply, control.takes));
        }
        events
    }
}

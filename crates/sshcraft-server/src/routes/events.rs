//! `GET /v1/events`: one WebSocket per device for change notices.

use crate::auth::{Session, bearer_token};
use crate::error::ApiError;
use crate::hub::Event;
use crate::state::AppState;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::Response;

pub async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let token = bearer_token(&headers)?;
    let session = Session::check(&state, &token).await?;
    Ok(upgrade.on_upgrade(move |socket| run(state, socket, session, token)))
}

/// Forwards events, pings every `ws_recheck` and checks the session again at each ping, so a
/// device blocked or locked from outside this process (the admin command) is cut off too.
async fn run(state: AppState, mut socket: WebSocket, session: Session, token: Vec<u8>) {
    let (subscription, mut events) = state.hub.subscribe(session.device_id, session.account);
    let mut tick = tokio::time::interval(state.ws_recheck);
    tick.tick().await;
    loop {
        tokio::select! {
            event = events.recv() => {
                let Some(event) = event else { break };
                let ended = event == Event::SessionEnded;
                if send(&mut socket, &event).await.is_err() || ended {
                    break;
                }
            }
            _ = tick.tick() => {
                if Session::check(&state, &token).await.is_err() {
                    let _ = send(&mut socket, &Event::SessionEnded).await;
                    break;
                }
                if socket.send(Message::Ping(Default::default())).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => match incoming {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                Some(Ok(_)) => {}
            },
        }
    }
    state.hub.unsubscribe(session.device_id, subscription);
    let _ = socket.send(Message::Close(None)).await;
}

async fn send(socket: &mut WebSocket, event: &Event) -> Result<(), axum::Error> {
    let text = serde_json::to_string(event).unwrap_or_default();
    socket.send(Message::Text(text.into())).await
}

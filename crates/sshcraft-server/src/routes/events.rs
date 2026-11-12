//! `GET /v1/events`: one WebSocket per device for change notices.

use crate::auth::{Session, bearer_token};
use crate::error::ApiError;
use crate::hub::{Event, MAX_PER_DEVICE};
use crate::state::AppState;
use axum::extract::State;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::Response;
use std::time::Duration;

/// Largest message or frame a client may send; clients only answer pings.
pub const MAX_CLIENT_MESSAGE: usize = 4096;

pub async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, ApiError> {
    let token = bearer_token(&headers)?;
    let session = Session::check(&state, &token).await?;
    let upgrade = upgrade.map_err(|_| ApiError::Invalid("websocket upgrade expected".into()))?;
    if state.hub.connections(session.device_id) >= MAX_PER_DEVICE {
        return Err(ApiError::Busy);
    }
    Ok(upgrade
        .max_message_size(MAX_CLIENT_MESSAGE)
        .max_frame_size(MAX_CLIENT_MESSAGE)
        .on_upgrade(move |socket| run(state, socket, session, token)))
}

/// Forwards events, pings every `ws_recheck` and checks the session again at each ping, so a
/// device blocked or locked from outside this process (the admin command) is cut off too.
/// Every send has the same time limit, so a client that stops reading cannot hold off the check.
async fn run(state: AppState, mut socket: WebSocket, session: Session, token: Vec<u8>) {
    let Some((subscription, mut events)) = state.hub.subscribe(session.device_id, session.account)
    else {
        let _ = socket.send(Message::Close(None)).await;
        return;
    };
    let limit = state.ws_recheck;
    let mut tick = tokio::time::interval(limit);
    tick.tick().await;
    loop {
        tokio::select! {
            event = events.recv() => {
                let Some(event) = event else { break };
                let ended = event == Event::SessionEnded;
                if send(&mut socket, &event, limit).await.is_err() || ended {
                    break;
                }
            }
            _ = tick.tick() => {
                if Session::check(&state, &token).await.is_err() {
                    let _ = send(&mut socket, &Event::SessionEnded, limit).await;
                    break;
                }
                let ping = socket.send(Message::Ping(Default::default()));
                if !matches!(tokio::time::timeout(limit, ping).await, Ok(Ok(()))) {
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
    let _ = tokio::time::timeout(limit, socket.send(Message::Close(None))).await;
}

async fn send(socket: &mut WebSocket, event: &Event, limit: Duration) -> Result<(), ()> {
    let text = serde_json::to_string(event).unwrap_or_default();
    match tokio::time::timeout(limit, socket.send(Message::Text(text.into()))).await {
        Ok(Ok(())) => Ok(()),
        _ => Err(()),
    }
}

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::{domain::WebSocketSpec, state::AppState};

pub async fn ensure_started(app: AppHandle, state: AppState, node_id: String, spec: WebSocketSpec) {
    {
        let mut started = state.websocket_started.lock().await;
        if !started.insert(node_id.clone()) {
            return;
        }
    }
    tauri::async_runtime::spawn(async move {
        let mut delay = 1u64;
        loop {
            let enabled = state
                .repository
                .get_stored_node(&node_id)
                .await
                .map(|node| node.enabled)
                .unwrap_or(false);
            if !enabled {
                break;
            }
            match connect_async(&spec.url).await {
                Ok((mut socket, _)) => {
                    delay = 1;
                    if let Some(Ok(Message::Text(initial))) = socket.next().await {
                        if let Ok(value) = serde_json::from_str::<Value>(&initial) {
                            if let Some(uid) = value.get("uid").and_then(Value::as_str) {
                                for topic in &spec.topics {
                                    let _ = socket
                                        .send(Message::Text(
                                            json!({"uid": uid, "subscribe": topic})
                                                .to_string()
                                                .into(),
                                        ))
                                        .await;
                                }
                            }
                        }
                    }
                    {
                        let mut runtime = state.runtime.lock().await;
                        runtime
                            .entry(node_id.clone())
                            .or_default()
                            .websocket_connected = true;
                    }
                    let _ = app.emit("monitor-state-changed", &node_id);
                    while let Some(message) = socket.next().await {
                        match message {
                            Ok(Message::Text(_)) | Ok(Message::Binary(_)) => {
                                let _ = app.emit("node-updated", &node_id);
                            }
                            Ok(Message::Ping(data)) => {
                                let _ = socket.send(Message::Pong(data)).await;
                            }
                            Ok(Message::Close(_)) | Err(_) => break,
                            _ => {}
                        }
                    }
                    if let Some(runtime) = state.runtime.lock().await.get_mut(&node_id) {
                        runtime.websocket_connected = false;
                    }
                    let _ = app.emit("monitor-state-changed", &node_id);
                }
                Err(_) => {
                    if let Some(runtime) = state.runtime.lock().await.get_mut(&node_id) {
                        runtime.websocket_connected = false;
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(delay)).await;
            delay = (delay * 2).min(60);
        }
        state.websocket_started.lock().await.remove(&node_id);
    });
}

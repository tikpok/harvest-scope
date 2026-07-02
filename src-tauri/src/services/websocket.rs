use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::{
    domain::{NetworkEvent, WebSocketSpec},
    state::AppState,
};

fn value_string(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

fn parse_network_event(node_id: &str, text: &str) -> Option<NetworkEvent> {
    let root = serde_json::from_str::<Value>(text).ok()?;
    let kind = root.get("topic")?.as_str()?;
    if kind != "block" && kind != "finalizedBlock" {
        return None;
    }
    let data = root.get("data").unwrap_or(&root);
    let block = data.get("block").unwrap_or(data);
    let height = value_string(block.get("height").or_else(|| data.get("height")));
    let hash = value_string(
        data.get("meta")
            .and_then(|meta| meta.get("hash"))
            .or_else(|| data.get("hash"))
            .or_else(|| block.get("hash")),
    );
    Some(NetworkEvent {
        id: uuid::Uuid::new_v4().to_string(),
        node_id: node_id.to_owned(),
        kind: kind.to_owned(),
        height,
        hash,
        observed_at: chrono::Utc::now().timestamp_millis(),
        source: "websocket".into(),
    })
}

pub async fn publish_event(app: &AppHandle, state: &AppState, event: NetworkEvent) {
    {
        let mut events = state.network_events.lock().await;
        if events.len() >= 500 {
            events.pop_front();
        }
        events.push_back(event.clone());
    }
    let _ = app.emit("network-event", event);
}

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
                            Ok(Message::Text(text)) => {
                                if let Some(event) = parse_network_event(&node_id, &text) {
                                    publish_event(&app, &state, event).await;
                                }
                                let _ = app.emit("node-updated", &node_id);
                            }
                            Ok(Message::Binary(_)) => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_block_event() {
        let event = parse_network_event(
            "node-a",
            r#"{"topic":"block","data":{"meta":{"hash":"ABC"},"block":{"height":"123"}}}"#,
        )
        .expect("block event should parse");
        assert_eq!(event.node_id, "node-a");
        assert_eq!(event.kind, "block");
        assert_eq!(event.height.as_deref(), Some("123"));
        assert_eq!(event.hash.as_deref(), Some("ABC"));
    }

    #[test]
    fn ignores_non_network_messages() {
        assert!(parse_network_event("node-a", r#"{"uid":"socket-id"}"#).is_none());
    }
}

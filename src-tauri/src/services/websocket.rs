use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
};

use crate::{
    domain::{HarvestEvent, NetworkEvent, WebSocketSpec},
    state::AppState,
    storage::HarvestEventInsertOutcome,
};

fn value_string(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

fn valid_decimal(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_hex_256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_network_event_checked(
    node_id: &str,
    text: &str,
) -> Result<Option<NetworkEvent>, &'static str> {
    let root = serde_json::from_str::<Value>(text).map_err(|_| "JSON形式が不正です")?;
    let Some(kind) = root.get("topic").and_then(Value::as_str) else {
        return Ok(None);
    };
    if kind != "block" && kind != "finalizedBlock" {
        return Ok(None);
    }
    let data = root.get("data").unwrap_or(&root);
    let block = data.get("block").unwrap_or(data);
    let height = value_string(block.get("height").or_else(|| data.get("height")));
    if height.as_deref().is_some_and(|value| !valid_decimal(value)) {
        return Err("heightが不正です");
    }
    let hash = value_string(
        data.get("meta")
            .and_then(|meta| meta.get("hash"))
            .or_else(|| data.get("hash"))
            .or_else(|| block.get("hash")),
    );
    if hash.as_deref().is_some_and(|value| !valid_hex_256(value)) {
        return Err("hashが64桁hexではありません");
    }
    let signer_public_key = value_string(
        block
            .get("signerPublicKey")
            .or_else(|| data.get("signerPublicKey"))
            .or_else(|| {
                block
                    .get("signer")
                    .and_then(|signer| signer.get("publicKey"))
            }),
    )
    .map(|value| value.to_ascii_uppercase());
    if signer_public_key
        .as_deref()
        .is_some_and(|value| !valid_hex_256(value))
    {
        return Err("signerPublicKeyが64桁hexではありません");
    }
    Ok(Some(NetworkEvent {
        id: uuid::Uuid::new_v4().to_string(),
        node_id: node_id.to_owned(),
        kind: kind.to_owned(),
        height,
        hash,
        signer_public_key,
        harvester_public_key: None,
        harvested_node_id: None,
        observer_node_id: None,
        observed_at: chrono::Utc::now().timestamp_millis(),
        source: "websocket".into(),
    }))
}

#[cfg(test)]
fn parse_network_event(node_id: &str, text: &str) -> Option<NetworkEvent> {
    parse_network_event_checked(node_id, text).ok().flatten()
}

async fn note_transport_rejection(state: &AppState, node_id: &str, reason: impl Into<String>) {
    let reason = reason.into();
    let mut runtime = state.runtime.lock().await;
    let runtime = runtime.entry(node_id.to_owned()).or_default();
    runtime.transport_rejected_count = runtime.transport_rejected_count.saturating_add(1);
    runtime.last_transport_rejection_at = Some(chrono::Utc::now().timestamp_millis());
    runtime.last_transport_rejection_reason = Some(reason.clone());
    if runtime.transport_rejected_count.is_power_of_two() {
        tracing::warn!(node_id, count = runtime.transport_rejected_count, %reason, "rejected node transport input");
    }
}

async fn publish_harvest_events(app: &AppHandle, state: &AppState, event: &NetworkEvent) {
    if event.kind != "block" {
        return;
    }
    let Some(signer_public_key) = event.signer_public_key.as_ref() else {
        return;
    };
    let Ok(node_ids) = state
        .repository
        .harvest_nodes_for_public_key(signer_public_key)
        .await
    else {
        return;
    };
    for node_id in node_ids {
        let harvest = HarvestEvent {
            id: uuid::Uuid::new_v4().to_string(),
            node_id: node_id.clone(),
            observer_node_id: event.node_id.clone(),
            harvester_public_key: signer_public_key.clone(),
            height: event.height.clone(),
            hash: event.hash.clone(),
            observed_at: event.observed_at,
        };
        let raw_retention_days = state.settings.read().await.raw_retention_days;
        let Ok(outcome) = state
            .repository
            .record_harvest_event(&harvest, raw_retention_days)
            .await
        else {
            continue;
        };
        match outcome {
            HarvestEventInsertOutcome::Inserted => {}
            HarvestEventInsertOutcome::Duplicate => continue,
            HarvestEventInsertOutcome::RateLimited => {
                note_transport_rejection(state, &node_id, "harvest_raw_rate_limit").await;
                continue;
            }
            HarvestEventInsertOutcome::RowLimitReached => {
                note_transport_rejection(state, &node_id, "harvest_raw_row_limit").await;
                continue;
            }
            HarvestEventInsertOutcome::DatabaseSizeLimitReached => {
                note_transport_rejection(state, &node_id, "harvest_raw_database_size_limit").await;
                continue;
            }
        }
        publish_event(
            app,
            state,
            NetworkEvent {
                id: uuid::Uuid::new_v4().to_string(),
                node_id: node_id.clone(),
                kind: "harvestedBlock".into(),
                height: event.height.clone(),
                hash: event.hash.clone(),
                signer_public_key: event.signer_public_key.clone(),
                harvester_public_key: Some(signer_public_key.clone()),
                harvested_node_id: Some(node_id),
                observer_node_id: Some(event.node_id.clone()),
                observed_at: event.observed_at,
                source: "websocket".into(),
            },
        )
        .await;
    }
}

pub async fn publish_event(app: &AppHandle, state: &AppState, event: NetworkEvent) {
    {
        let mut events = state.network_events.lock().await;
        if !events.push(event.clone()) {
            drop(events);
            note_transport_rejection(state, &event.node_id, "イベントバッファ上限を超えました")
                .await;
            return;
        }
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
            let mut received_valid_event = false;
            let enabled = state
                .repository
                .get_stored_node(&node_id)
                .await
                .map(|node| node.enabled)
                .unwrap_or(false);
            if !enabled {
                break;
            }
            let mut config = WebSocketConfig::default();
            config.max_message_size = Some(256 * 1024);
            config.max_frame_size = Some(64 * 1024);
            match connect_async_with_config(&spec.url, Some(config), false).await {
                Ok((mut socket, _)) => {
                    let uid = match socket.next().await {
                        Some(Ok(Message::Text(initial))) => serde_json::from_str::<Value>(&initial)
                            .ok()
                            .and_then(|value| {
                                value.get("uid").and_then(Value::as_str).map(str::to_owned)
                            })
                            .filter(|uid| !uid.is_empty() && uid.len() <= 128),
                        _ => None,
                    };
                    if let Some(uid) = uid {
                        for topic in &spec.topics {
                            let _ = socket
                                .send(Message::Text(
                                    json!({"uid": uid, "subscribe": topic}).to_string().into(),
                                ))
                                .await;
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
                                    match parse_network_event_checked(&node_id, &text) {
                                        Ok(Some(event)) => {
                                            received_valid_event = true;
                                            publish_harvest_events(&app, &state, &event).await;
                                            publish_event(&app, &state, event).await;
                                        }
                                        Ok(None) => {}
                                        Err(reason) => {
                                            note_transport_rejection(&state, &node_id, reason)
                                                .await;
                                        }
                                    }
                                    let _ = app.emit("node-updated", &node_id);
                                }
                                Ok(Message::Binary(_)) => {
                                    note_transport_rejection(
                                        &state,
                                        &node_id,
                                        "binary通知は受理しません",
                                    )
                                    .await;
                                }
                                Ok(Message::Ping(data)) => {
                                    let _ = socket.send(Message::Pong(data)).await;
                                }
                                Ok(Message::Close(_)) => break,
                                Err(error) => {
                                    note_transport_rejection(
                                        &state,
                                        &node_id,
                                        format!("WebSocket制限違反: {error}"),
                                    )
                                    .await;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    } else {
                        note_transport_rejection(
                            &state,
                            &node_id,
                            "初期UIDが不正または128 bytes超です",
                        )
                        .await;
                        let _ = socket.close(None).await;
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
            if received_valid_event {
                delay = 1;
            }
            tokio::time::sleep(Duration::from_secs(delay)).await;
            if !received_valid_event {
                delay = (delay * 2).min(60);
            }
        }
        state.websocket_started.lock().await.remove(&node_id);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_block_event() {
        let hash = "A".repeat(64);
        let event = parse_network_event(
            "node-a",
            &format!(r#"{{"topic":"block","data":{{"meta":{{"hash":"{hash}"}},"block":{{"height":"123"}}}}}}"#),
        )
        .expect("block event should parse");
        assert_eq!(event.node_id, "node-a");
        assert_eq!(event.kind, "block");
        assert_eq!(event.height.as_deref(), Some("123"));
        assert_eq!(event.hash.as_deref(), Some(hash.as_str()));
    }

    #[test]
    fn parses_block_signer_public_key() {
        let key = "a".repeat(64);
        let event = parse_network_event(
            "node-a",
            &format!(r#"{{"topic":"block","data":{{"block":{{"height":"123","signerPublicKey":"{key}"}}}}}}"#),
        )
        .expect("block event should parse");

        assert_eq!(
            event.signer_public_key.as_deref(),
            Some("A".repeat(64).as_str())
        );
    }

    #[test]
    fn ignores_non_network_messages() {
        assert!(parse_network_event("node-a", r#"{"uid":"socket-id"}"#).is_none());
    }

    #[test]
    fn rejects_malformed_network_event_fields() {
        assert!(parse_network_event(
            "node-a",
            r#"{"topic":"block","data":{"meta":{"hash":"ABC"},"block":{"height":"123","signerPublicKey":"xyz"}}}"#,
        )
        .is_none());
        assert!(parse_network_event(
            "node-a",
            r#"{"topic":"block","data":{"block":{"height":"123456789012345678901"}}}"#,
        )
        .is_none());
    }
}

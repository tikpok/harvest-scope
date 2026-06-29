use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::{
    domain::{
        ChainSnapshot, HealthSnapshot, NodeAdapter, NodeIdentity, NodeKind, PeerInfo, WebSocketSpec,
    },
    error::{AppError, AppResult},
};

use super::{string_value, u64_value};

pub struct SymbolAdapter {
    base_url: String,
    client: Client,
}

impl SymbolAdapter {
    pub fn new(base_url: String, client: Client) -> Self {
        Self { base_url, client }
    }

    async fn get_json(&self, path: &str) -> AppResult<Value> {
        let response = self
            .client
            .get(format!("{}{}", self.base_url, path))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(AppError::Network(format!(
                "HTTP {} ({path})",
                response.status()
            )));
        }
        response.json().await.map_err(Into::into)
    }

    fn version(raw: Option<u64>) -> Option<String> {
        raw.map(|value| {
            let bytes = (value as u32).to_be_bytes();
            format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])
        })
    }

    fn roles(raw: u64) -> Vec<String> {
        [
            (1, "Peer"),
            (2, "API"),
            (4, "Voting"),
            (8, "IPv4"),
            (16, "IPv6"),
        ]
        .into_iter()
        .filter(|(bit, _)| raw & bit != 0)
        .map(|(_, name)| name.to_string())
        .collect()
    }
}

#[async_trait]
impl NodeAdapter for SymbolAdapter {
    async fn probe(&self) -> AppResult<NodeIdentity> {
        let info = self.get_json("/node/info").await?;
        let public_key = info.get("publicKey").and_then(string_value);
        let generation_hash = info.get("networkGenerationHashSeed").and_then(string_value);
        if public_key.is_none() && generation_hash.is_none() {
            return Err(AppError::InvalidPayload(
                "Symbol node/infoではありません".into(),
            ));
        }
        let server = self.get_json("/node/server").await.ok();
        let network_id = u64_value(info.get("networkIdentifier"));
        let network = network_id.map(|id| format!("Symbol-{id}")).or_else(|| {
            generation_hash
                .as_ref()
                .map(|hash| format!("Symbol-{}", &hash[..hash.len().min(8)]))
        });
        Ok(NodeIdentity {
            kind: NodeKind::Symbol,
            name: info
                .get("friendlyName")
                .and_then(string_value)
                .filter(|name| !name.is_empty()),
            network,
            version: Self::version(u64_value(info.get("version"))),
            public_key,
            roles: Self::roles(u64_value(info.get("roles")).unwrap_or_default()),
            rest_version: server
                .as_ref()
                .and_then(|value| value.get("restVersion"))
                .and_then(string_value),
        })
    }

    async fn fetch_health(&self) -> AppResult<HealthSnapshot> {
        let response = self
            .client
            .get(format!("{}/node/health", self.base_url))
            .send()
            .await?;
        let status_code = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        let status = body.get("status").unwrap_or(&body);
        let api_status = status.get("apiNode").and_then(string_value);
        let db_status = status.get("db").and_then(string_value);
        let healthy = status_code.is_success()
            && api_status
                .as_deref()
                .map_or(true, |value| value.eq_ignore_ascii_case("up"))
            && db_status
                .as_deref()
                .map_or(true, |value| value.eq_ignore_ascii_case("up"));
        Ok(HealthSnapshot {
            reachable: true,
            api_status,
            db_status,
            nis_status_code: None,
            healthy,
        })
    }

    async fn fetch_chain(&self) -> AppResult<ChainSnapshot> {
        let chain = self.get_json("/chain/info").await?;
        let height = u64_value(chain.get("height"));
        if height.is_none() {
            return Err(AppError::InvalidPayload(
                "chain/infoにheightがありません".into(),
            ));
        }
        let finalized_height = chain
            .get("latestFinalizedBlock")
            .and_then(|value| u64_value(value.get("height")));
        let score_high = u64_value(chain.get("scoreHigh"));
        let score_low = u64_value(chain.get("scoreLow"));
        let chain_score = match (score_high, score_low) {
            (Some(high), Some(low)) => Some(((high as u128) << 64 | low as u128).to_string()),
            _ => None,
        };
        Ok(ChainSnapshot {
            height,
            finalized_height,
            network_height: None,
            chain_score,
        })
    }

    async fn fetch_peers(&self) -> AppResult<Vec<PeerInfo>> {
        let peers = self.get_json("/node/peers").await?;
        let items = peers
            .as_array()
            .ok_or_else(|| AppError::InvalidPayload("node/peersが配列ではありません".into()))?;
        Ok(items
            .iter()
            .enumerate()
            .map(|(index, peer)| {
                let public_key = peer.get("publicKey").and_then(string_value);
                let host = peer.get("host").and_then(string_value);
                PeerInfo {
                    key: public_key
                        .clone()
                        .or_else(|| host.clone())
                        .unwrap_or_else(|| format!("peer-{index}")),
                    name: peer.get("friendlyName").and_then(string_value),
                    host,
                    port: u64_value(peer.get("port")).and_then(|value| u16::try_from(value).ok()),
                    status: "active".into(),
                    version: Self::version(u64_value(peer.get("version"))),
                }
            })
            .collect())
    }

    async fn fetch_state_hash(&self, height: u64) -> AppResult<Option<String>> {
        let block = self.get_json(&format!("/blocks/{height}")).await?;
        Ok(block
            .get("block")
            .unwrap_or(&block)
            .get("stateHash")
            .and_then(string_value))
    }

    fn websocket_capability(&self) -> Option<WebSocketSpec> {
        let url = self
            .base_url
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1);
        Some(WebSocketSpec {
            url: format!("{url}/ws"),
            topics: vec!["block".into(), "finalizedBlock".into()],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_symbol_version() {
        assert_eq!(
            SymbolAdapter::version(Some(0x01000304)).as_deref(),
            Some("1.0.3.4")
        );
    }
}

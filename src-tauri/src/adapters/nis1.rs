use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::{
    domain::{ChainSnapshot, HealthSnapshot, NodeAdapter, NodeIdentity, NodeKind, PeerInfo},
    error::{AppError, AppResult},
};

use super::{string_value, u64_value};

pub struct Nis1Adapter {
    base_url: String,
    client: Client,
}

impl Nis1Adapter {
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

    fn parse_node(root: &Value) -> AppResult<NodeIdentity> {
        let node = root.get("node").unwrap_or(root);
        let metadata = node
            .get("metaData")
            .or_else(|| node.get("metadata"))
            .unwrap_or(&Value::Null);
        let identity = node.get("identity").unwrap_or(&Value::Null);
        let application = metadata.get("application").and_then(string_value);
        if application
            .as_deref()
            .map_or(true, |value| !value.eq_ignore_ascii_case("NIS"))
        {
            return Err(AppError::InvalidPayload(
                "NIS node infoではありません".into(),
            ));
        }
        let network_id = metadata
            .get("networkId")
            .and_then(|value| value.as_i64())
            .or_else(|| u64_value(metadata.get("networkId")).map(|value| value as i64));
        Ok(NodeIdentity {
            kind: NodeKind::Nis1,
            name: identity.get("name").and_then(string_value),
            network: network_id.map(|id| match id as i8 {
                104 => "NEM Mainnet".into(),
                -104 => "NEM Testnet".into(),
                96 => "Mijin".into(),
                _ => format!("NEM-{id}"),
            }),
            version: metadata.get("version").and_then(string_value),
            public_key: identity
                .get("public-key")
                .or_else(|| identity.get("publicKey"))
                .and_then(string_value),
            roles: vec!["NIS".into()],
            rest_version: root
                .get("nisInfo")
                .and_then(|info| info.get("version"))
                .and_then(string_value),
        })
    }
}

#[async_trait]
impl NodeAdapter for Nis1Adapter {
    async fn probe(&self) -> AppResult<NodeIdentity> {
        match self.get_json("/node/extended-info").await {
            Ok(value) => Self::parse_node(&value),
            Err(_) => Self::parse_node(&self.get_json("/node/info").await?),
        }
    }

    async fn fetch_health(&self) -> AppResult<HealthSnapshot> {
        let heartbeat = self.get_json("/heartbeat").await?;
        if heartbeat.get("code").and_then(|value| value.as_i64()) != Some(1) {
            return Err(AppError::InvalidPayload(
                "heartbeatが成功を返しませんでした".into(),
            ));
        }
        let status = self.get_json("/status").await?;
        let code = status.get("code").and_then(|value| value.as_i64());
        Ok(HealthSnapshot {
            reachable: true,
            api_status: None,
            db_status: None,
            nis_status_code: code,
            healthy: code == Some(6),
        })
    }

    async fn fetch_chain(&self) -> AppResult<ChainSnapshot> {
        let height_json = self.get_json("/chain/height").await?;
        let height = u64_value(height_json.get("height"));
        if height.is_none() {
            return Err(AppError::InvalidPayload(
                "chain/heightにheightがありません".into(),
            ));
        }
        let score = self
            .get_json("/chain/score")
            .await
            .ok()
            .and_then(|value| value.get("score").and_then(string_value));
        let network_height = self
            .get_json("/node/active-peers/max-chain-height")
            .await
            .ok()
            .and_then(|value| u64_value(value.get("height")));
        Ok(ChainSnapshot {
            height,
            finalized_height: None,
            network_height,
            chain_score: score,
        })
    }

    async fn fetch_peers(&self) -> AppResult<Vec<PeerInfo>> {
        let root = self.get_json("/node/peer-list/all").await?;
        let mut peers = Vec::new();
        for status in ["active", "inactive", "busy", "failure"] {
            for (index, peer) in root
                .get(status)
                .and_then(|value| value.as_array())
                .into_iter()
                .flatten()
                .enumerate()
            {
                let identity = peer.get("identity").unwrap_or(&Value::Null);
                let endpoint = peer.get("endpoint").unwrap_or(&Value::Null);
                let metadata = peer
                    .get("metaData")
                    .or_else(|| peer.get("metadata"))
                    .unwrap_or(&Value::Null);
                let key = identity
                    .get("public-key")
                    .or_else(|| identity.get("publicKey"))
                    .and_then(string_value);
                let host = endpoint.get("host").and_then(string_value);
                peers.push(PeerInfo {
                    key: key
                        .or_else(|| host.clone())
                        .unwrap_or_else(|| format!("{status}-{index}")),
                    name: identity.get("name").and_then(string_value),
                    host,
                    port: u64_value(endpoint.get("port"))
                        .and_then(|value| u16::try_from(value).ok()),
                    status: status.into(),
                    version: metadata.get("version").and_then(string_value),
                });
            }
        }
        Ok(peers)
    }
}

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

    fn node_identity(root: &Value) -> NodeIdentity {
        let node = root.get("node").unwrap_or(root);
        let metadata = node
            .get("metaData")
            .or_else(|| node.get("metadata"))
            .unwrap_or(&Value::Null);
        let identity = node.get("identity").unwrap_or(&Value::Null);
        let network_id = metadata
            .get("networkId")
            .and_then(|value| value.as_i64())
            .or_else(|| u64_value(metadata.get("networkId")).map(|value| value as i64));
        NodeIdentity {
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
        }
    }

    fn parse_node(root: &Value) -> AppResult<NodeIdentity> {
        let node = root.get("node").unwrap_or(root);
        let metadata = node
            .get("metaData")
            .or_else(|| node.get("metadata"))
            .unwrap_or(&Value::Null);
        let applications = [
            metadata.get("application"),
            root.get("nisInfo").and_then(|info| info.get("application")),
        ];
        let is_nis = applications.into_iter().flatten().any(|value| {
            string_value(value)
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case("NIS"))
        });
        if !is_nis {
            return Err(AppError::InvalidPayload(
                "NIS node infoではありません".into(),
            ));
        }
        Ok(Self::node_identity(root))
    }
}

#[async_trait]
impl NodeAdapter for Nis1Adapter {
    async fn probe(&self) -> AppResult<NodeIdentity> {
        let mut node_payloads = Vec::new();
        let mut probe_errors = Vec::new();
        for path in ["/node/extended-info", "/node/info"] {
            match self.get_json(path).await {
                Ok(value) => match Self::parse_node(&value) {
                    Ok(identity) => return Ok(identity),
                    Err(error) => {
                        probe_errors.push(format!("{path}: {error}"));
                        node_payloads.push(value);
                    }
                },
                Err(error) => probe_errors.push(format!("{path}: {error}")),
            }
        }

        // Some NIS1 installations omit the application marker or restrict the
        // node information endpoints. /heartbeat is NIS1-specific and gives us
        // a safe fallback without mistaking Symbol's /node/info for NIS1.
        match self.get_json("/heartbeat").await {
            Ok(heartbeat) if heartbeat.get("code").and_then(Value::as_i64) == Some(1) => {
                return Ok(node_payloads
                    .first()
                    .map(Self::node_identity)
                    .unwrap_or_else(|| Self::node_identity(&Value::Null)));
            }
            Ok(_) => probe_errors.push("/heartbeat: code=1 ではありません".into()),
            Err(error) => probe_errors.push(format!("/heartbeat: {error}")),
        }

        Err(AppError::InvalidPayload(probe_errors.join("; ")))
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

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[test]
    fn parses_extended_info_fixture() {
        let root: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/nis1/node-extended-info.json"
        ))
        .expect("fixture should be valid JSON");

        let identity = Nis1Adapter::parse_node(&root).expect("fixture should be NIS1");

        assert_eq!(identity.kind, NodeKind::Nis1);
        assert_eq!(identity.network.as_deref(), Some("NEM Mainnet"));
        assert_eq!(identity.version.as_deref(), Some("0.6.101"));
    }

    #[test]
    fn accepts_nis_info_application_marker() {
        let root = serde_json::json!({
            "node": {
                "metaData": { "networkId": 104, "version": "0.6.101" },
                "identity": { "name": "NIS1" }
            },
            "nisInfo": { "application": "NIS", "version": "0.6.101" }
        });

        let identity = Nis1Adapter::parse_node(&root).expect("nisInfo identifies NIS1");

        assert_eq!(identity.name.as_deref(), Some("NIS1"));
        assert_eq!(identity.rest_version.as_deref(), Some("0.6.101"));
    }

    #[tokio::test]
    async fn heartbeat_fallback_detects_markerless_nis1() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock server should bind");
        let address = listener.local_addr().expect("mock server has an address");
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.expect("request should arrive");
                tokio::spawn(async move {
                    let mut request = [0_u8; 2048];
                    let read = socket
                        .read(&mut request)
                        .await
                        .expect("request should be read");
                    let request = String::from_utf8_lossy(&request[..read]);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let (status, body) = match path {
                        "/node/extended-info" => (404, "{}"),
                        "/node/info" => (
                            200,
                            r#"{"metaData":{"networkId":104,"version":"0.6.101"},"identity":{"name":"legacy NIS1"}}"#,
                        ),
                        "/heartbeat" => (200, r#"{"code":1,"type":2,"message":"ok"}"#),
                        _ => (404, "{}"),
                    };
                    let reason = if status == 200 { "OK" } else { "Not Found" };
                    let response = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    socket
                        .write_all(response.as_bytes())
                        .await
                        .expect("response should be written");
                });
            }
        });
        let adapter = Nis1Adapter::new(format!("http://{address}"), Client::new());

        let identity = adapter
            .probe()
            .await
            .expect("heartbeat should identify NIS1");
        server.abort();

        assert_eq!(identity.kind, NodeKind::Nis1);
        assert_eq!(identity.name.as_deref(), Some("legacy NIS1"));
        assert_eq!(identity.network.as_deref(), Some("NEM Mainnet"));
    }
}

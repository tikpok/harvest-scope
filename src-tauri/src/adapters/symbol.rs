use std::collections::HashSet;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::{
    domain::{
        ChainSnapshot, HarvestSnapshot, HealthSnapshot, NodeAdapter, NodeIdentity, NodeKind,
        PeerInfo, WebSocketSpec,
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

    async fn post_json(&self, path: &str, body: Value) -> AppResult<Value> {
        let response = self
            .client
            .post(format!("{}{}", self.base_url, path))
            .json(&body)
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

    fn network_label(raw: Option<&Value>) -> Option<String> {
        let id = raw.and_then(|value| match value {
            Value::String(value) => value.parse::<i64>().ok(),
            Value::Number(value) => value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok())),
            _ => None,
        })?;
        Some(match id {
            -104 | 104 => "SYMBOL Mainnet".into(),
            152 => "SYMBOL Testnet".into(),
            _ => format!("SYMBOL-{id}"),
        })
    }

    fn rest_version(server: Option<&Value>) -> Option<String> {
        server.and_then(|server| {
            server
                .get("restVersion")
                .or_else(|| {
                    server
                        .get("serverInfo")
                        .and_then(|info| info.get("restVersion"))
                })
                .and_then(string_value)
        })
    }

    fn push_public_keys(value: &Value, keys: &mut Vec<String>) {
        match value {
            Value::String(value) if !value.trim().is_empty() => {
                keys.push(value.trim().to_ascii_uppercase());
            }
            Value::Array(values) => {
                for value in values {
                    Self::push_public_keys(value, keys);
                }
            }
            Value::Object(object) => {
                for key in [
                    "unlockedAccount",
                    "unlockedAccounts",
                    "publicKeys",
                    "publicKey",
                    "data",
                ] {
                    if let Some(value) = object.get(key) {
                        Self::push_public_keys(value, keys);
                    }
                }
            }
            _ => {}
        }
    }

    fn normalize_mosaic_id(value: &str) -> String {
        let normalized = value
            .trim()
            .trim_matches('\'')
            .trim_matches('"')
            .replace(['\'', '"', '_', ' '], "")
            .replace("`", "")
            .trim_start_matches("0x")
            .trim_start_matches("0X")
            .to_ascii_uppercase();
        if normalized
            .chars()
            .all(|character| character.is_ascii_digit())
            && normalized.len() > 16
        {
            normalized
                .parse::<u128>()
                .ok()
                .map(|value| format!("{value:016X}"))
                .unwrap_or(normalized)
        } else {
            normalized
        }
    }

    fn u128_string(value: Option<&Value>) -> Option<u128> {
        value.and_then(|value| match value {
            Value::String(value) => value.parse().ok(),
            Value::Number(value) => value.as_u64().map(u128::from),
            _ => None,
        })
    }

    fn account_items(root: &Value) -> Vec<&Value> {
        root.get("data")
            .and_then(Value::as_array)
            .or_else(|| root.as_array())
            .map(|items| items.iter().collect())
            .unwrap_or_default()
    }

    fn account_body(item: &Value) -> &Value {
        item.get("account").unwrap_or(item)
    }

    fn account_public_key(account: &Value) -> Option<String> {
        account
            .get("publicKey")
            .and_then(string_value)
            .map(|value| value.to_ascii_uppercase())
            .filter(|value| !value.is_empty())
    }

    fn linked_public_key(account: &Value) -> Option<String> {
        account
            .get("supplementalPublicKeys")
            .and_then(|keys| keys.get("linked"))
            .and_then(|linked| linked.get("publicKey"))
            .and_then(string_value)
            .or_else(|| account.get("linkedPublicKey").and_then(string_value))
            .map(|value| value.to_ascii_uppercase())
            .filter(|value| !value.is_empty())
    }

    fn network_currency_mosaic_id(properties: &Value) -> Option<String> {
        properties
            .get("chain")
            .and_then(|chain| chain.get("currencyMosaicId"))
            .and_then(string_value)
            .map(|value| Self::normalize_mosaic_id(&value))
            .filter(|value| !value.is_empty())
    }

    fn account_balance(account: &Value, currency_mosaic_id: &str) -> u128 {
        let Some(mosaics) = account.get("mosaics").and_then(Value::as_array) else {
            return 0;
        };
        mosaics
            .iter()
            .filter(|mosaic| {
                mosaic
                    .get("id")
                    .and_then(string_value)
                    .map(|id| Self::normalize_mosaic_id(&id) == currency_mosaic_id)
                    .unwrap_or(false)
            })
            .filter_map(|mosaic| Self::u128_string(mosaic.get("amount")))
            .sum()
    }

    async fn fetch_accounts_by_public_keys(
        &self,
        public_keys: &[String],
        errors: &mut Vec<String>,
    ) -> Vec<Value> {
        let mut accounts = Vec::new();
        for chunk in public_keys.chunks(100) {
            let body = serde_json::json!({ "publicKeys": chunk });
            match self.post_json("/accounts", body).await {
                Ok(response) => {
                    accounts.extend(
                        Self::account_items(&response)
                            .into_iter()
                            .map(Self::account_body)
                            .cloned(),
                    );
                }
                Err(error) => errors.push(error.to_string()),
            }
        }
        accounts
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
        let network = Self::network_label(info.get("networkIdentifier")).or_else(|| {
            generation_hash
                .as_ref()
                .map(|hash| format!("SYMBOL-{}", &hash[..hash.len().min(8)]))
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
            rest_version: Self::rest_version(server.as_ref()),
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

    async fn fetch_harvest_metrics(&self) -> AppResult<HarvestSnapshot> {
        let unlocked = self.get_json("/node/unlockedaccount").await?;
        let mut harvester_public_keys = Vec::new();
        Self::push_public_keys(&unlocked, &mut harvester_public_keys);
        harvester_public_keys.sort();
        harvester_public_keys.dedup();

        let delegated_harvester_count = i64::try_from(harvester_public_keys.len()).ok();
        if harvester_public_keys.is_empty() {
            return Ok(HarvestSnapshot {
                supported: true,
                harvester_public_keys,
                delegated_harvester_count,
                total_importance: Some("0".into()),
                total_balance_micro_xym: Some("0".into()),
                error: None,
            });
        }

        let currency_mosaic_id = match self.get_json("/network/properties").await {
            Ok(properties) => Self::network_currency_mosaic_id(&properties),
            Err(error) => {
                return Ok(HarvestSnapshot {
                    supported: true,
                    harvester_public_keys,
                    delegated_harvester_count,
                    total_importance: None,
                    total_balance_micro_xym: None,
                    error: Some(format!("network/properties: {error}")),
                });
            }
        };

        let mut account_errors = Vec::new();
        let harvester_accounts = self
            .fetch_accounts_by_public_keys(&harvester_public_keys, &mut account_errors)
            .await;
        let mut linked_public_keys = harvester_accounts
            .iter()
            .filter_map(Self::linked_public_key)
            .collect::<Vec<_>>();
        linked_public_keys.sort();
        linked_public_keys.dedup();
        let linked_accounts = self
            .fetch_accounts_by_public_keys(&linked_public_keys, &mut account_errors)
            .await;
        let linked_account_keys = linked_accounts
            .iter()
            .filter_map(Self::account_public_key)
            .collect::<HashSet<_>>();

        let mut total_importance = 0u128;
        let mut total_balance = 0u128;
        for account in &linked_accounts {
            total_importance += Self::u128_string(account.get("importance")).unwrap_or_default();
            if let Some(currency_mosaic_id) = currency_mosaic_id.as_deref() {
                total_balance += Self::account_balance(account, currency_mosaic_id);
            }
        }
        for account in &harvester_accounts {
            if Self::linked_public_key(account)
                .as_ref()
                .is_some_and(|key| linked_account_keys.contains(key))
            {
                continue;
            }
            total_importance += Self::u128_string(account.get("importance")).unwrap_or_default();
            if let Some(currency_mosaic_id) = currency_mosaic_id.as_deref() {
                total_balance += Self::account_balance(account, currency_mosaic_id);
            }
        }

        Ok(HarvestSnapshot {
            supported: true,
            harvester_public_keys,
            delegated_harvester_count,
            total_importance: Some(total_importance.to_string()),
            total_balance_micro_xym: currency_mosaic_id.map(|_| total_balance.to_string()),
            error: (!account_errors.is_empty()).then(|| account_errors.join("; ")),
        })
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

    #[test]
    fn labels_symbol_network_and_nested_rest_version() {
        assert_eq!(
            SymbolAdapter::network_label(Some(&serde_json::json!(104))).as_deref(),
            Some("SYMBOL Mainnet")
        );
        assert_eq!(
            SymbolAdapter::network_label(Some(&serde_json::json!("-104"))).as_deref(),
            Some("SYMBOL Mainnet")
        );
        assert_eq!(
            SymbolAdapter::rest_version(Some(
                &serde_json::json!({"serverInfo": {"restVersion": "2.4.1"}})
            ))
            .as_deref(),
            Some("2.4.1")
        );
    }

    #[test]
    fn extracts_unlocked_account_keys() {
        let mut keys = Vec::new();
        SymbolAdapter::push_public_keys(
            &serde_json::json!({"unlockedAccount": ["abc", "DEF"], "data": [{"publicKey": "abc"}]}),
            &mut keys,
        );
        keys.sort();
        keys.dedup();

        assert_eq!(keys, ["ABC", "DEF"]);
    }

    #[test]
    fn parses_network_currency_mosaic_id() {
        let properties =
            serde_json::json!({"chain": {"currencyMosaicId": "0x6BED'913F'A202'23F8"}});
        assert_eq!(
            SymbolAdapter::network_currency_mosaic_id(&properties).as_deref(),
            Some("6BED913FA20223F8")
        );
    }

    #[test]
    fn sums_network_currency_balance() {
        let account = serde_json::json!({
            "mosaics": [
                { "id": "0x6BED'913F'A202'23F8", "amount": "12000000" },
                { "id": "0x1234", "amount": "99" }
            ]
        });

        assert_eq!(
            SymbolAdapter::account_balance(&account, "6BED913FA20223F8"),
            12_000_000
        );
    }

    #[test]
    fn extracts_linked_account_public_key() {
        let account = serde_json::json!({
            "publicKey": "REMOTE",
            "supplementalPublicKeys": {
                "linked": { "publicKey": "main" }
            }
        });

        assert_eq!(
            SymbolAdapter::linked_public_key(&account).as_deref(),
            Some("MAIN")
        );
    }
}

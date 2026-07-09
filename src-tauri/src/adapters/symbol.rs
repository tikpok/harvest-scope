use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use chrono::{NaiveDate, TimeZone, Utc};
use reqwest::Client;
use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    DigitallySignedStruct, SignatureScheme,
};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::{
    domain::{
        CertificateSnapshot, ChainSnapshot, HarvestSnapshot, HealthSnapshot, NodeAdapter,
        NodeIdentity, NodeKind, PeerInfo, WebSocketSpec,
    },
    error::{AppError, AppResult},
};

use super::{string_value, u64_value};

pub struct SymbolAdapter {
    base_url: String,
    client: Client,
}

#[derive(Debug)]
struct CertificateOnlyVerifier;

impl ServerCertVerifier for CertificateOnlyVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ED25519,
        ]
    }
}

struct DerReader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> DerReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn peek_tag(&self) -> Option<u8> {
        self.data.get(self.offset).copied()
    }

    fn read_tlv(&mut self) -> AppResult<(u8, &'a [u8])> {
        let tag = *self
            .data
            .get(self.offset)
            .ok_or_else(|| AppError::InvalidPayload("証明書DERが途中で終わっています".into()))?;
        self.offset += 1;
        let first = *self
            .data
            .get(self.offset)
            .ok_or_else(|| AppError::InvalidPayload("証明書DERの長さがありません".into()))?;
        self.offset += 1;
        let length = if first & 0x80 == 0 {
            first as usize
        } else {
            let bytes = (first & 0x7f) as usize;
            if bytes == 0 || bytes > 8 {
                return Err(AppError::InvalidPayload(
                    "証明書DERの長さ形式が不正です".into(),
                ));
            }
            let mut length = 0usize;
            for _ in 0..bytes {
                length = (length << 8)
                    | *self.data.get(self.offset).ok_or_else(|| {
                        AppError::InvalidPayload("証明書DERの長さが途中で終わっています".into())
                    })? as usize;
                self.offset += 1;
            }
            length
        };
        let end = self.offset.checked_add(length).ok_or_else(|| {
            AppError::InvalidPayload("証明書DERの長さが大きすぎます".into())
        })?;
        let value = self
            .data
            .get(self.offset..end)
            .ok_or_else(|| AppError::InvalidPayload("証明書DER値が途中で終わっています".into()))?;
        self.offset = end;
        Ok((tag, value))
    }

    fn expect(&mut self, expected: u8) -> AppResult<&'a [u8]> {
        let (tag, value) = self.read_tlv()?;
        if tag != expected {
            return Err(AppError::InvalidPayload(format!(
                "証明書DERで想定外のタグを検出しました: {tag:#x}"
            )));
        }
        Ok(value)
    }
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

    fn i64_value(value: Option<&Value>) -> Option<i64> {
        value.and_then(|value| match value {
            Value::String(value) => value.parse().ok(),
            Value::Number(value) => value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok())),
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

    fn voting_key_ranges(account: &Value) -> Vec<(i64, i64)> {
        fn visit(value: &Value, ranges: &mut Vec<(i64, i64)>) {
            match value {
                Value::Array(items) => {
                    for item in items {
                        visit(item, ranges);
                    }
                }
                Value::Object(object) => {
                    if let (Some(start), Some(end)) = (
                        SymbolAdapter::i64_value(object.get("startEpoch")),
                        SymbolAdapter::i64_value(object.get("endEpoch")),
                    ) {
                        ranges.push((start, end));
                    }
                    for key in ["supplementalPublicKeys", "voting", "publicKeys", "data"] {
                        if let Some(value) = object.get(key) {
                            visit(value, ranges);
                        }
                    }
                }
                _ => {}
            }
        }

        let mut ranges = Vec::new();
        visit(account, &mut ranges);
        ranges.sort_unstable();
        ranges.dedup();
        ranges
    }

    fn parse_asn1_time(tag: u8, value: &[u8]) -> AppResult<i64> {
        let raw = std::str::from_utf8(value)
            .map_err(|_| AppError::InvalidPayload("証明書時刻がUTF-8ではありません".into()))?;
        let raw = raw.trim_end_matches('Z');
        let (year, rest) = match tag {
            0x17 => {
                if raw.len() < 10 {
                    return Err(AppError::InvalidPayload("UTCTimeが短すぎます".into()));
                }
                let year = raw[0..2].parse::<i32>().map_err(|_| {
                    AppError::InvalidPayload("UTCTimeの年を解析できません".into())
                })?;
                (if year >= 50 { 1900 + year } else { 2000 + year }, &raw[2..])
            }
            0x18 => {
                if raw.len() < 12 {
                    return Err(AppError::InvalidPayload(
                        "GeneralizedTimeが短すぎます".into(),
                    ));
                }
                let year = raw[0..4].parse::<i32>().map_err(|_| {
                    AppError::InvalidPayload("GeneralizedTimeの年を解析できません".into())
                })?;
                (year, &raw[4..])
            }
            _ => {
                return Err(AppError::InvalidPayload(
                    "証明書時刻タグが不正です".into(),
                ))
            }
        };
        let month = rest[0..2]
            .parse::<u32>()
            .map_err(|_| AppError::InvalidPayload("証明書時刻の月を解析できません".into()))?;
        let day = rest[2..4]
            .parse::<u32>()
            .map_err(|_| AppError::InvalidPayload("証明書時刻の日を解析できません".into()))?;
        let hour = rest[4..6]
            .parse::<u32>()
            .map_err(|_| AppError::InvalidPayload("証明書時刻の時を解析できません".into()))?;
        let minute = rest[6..8]
            .parse::<u32>()
            .map_err(|_| AppError::InvalidPayload("証明書時刻の分を解析できません".into()))?;
        let second = rest
            .get(8..10)
            .unwrap_or("00")
            .parse::<u32>()
            .map_err(|_| AppError::InvalidPayload("証明書時刻の秒を解析できません".into()))?;
        let date = NaiveDate::from_ymd_opt(year, month, day)
            .and_then(|date| date.and_hms_opt(hour, minute, second))
            .ok_or_else(|| AppError::InvalidPayload("証明書時刻が不正です".into()))?;
        Ok(Utc.from_utc_datetime(&date).timestamp_millis())
    }

    fn parse_certificate_not_after_ms(der: &[u8]) -> AppResult<i64> {
        let mut certificate = DerReader::new(der);
        let certificate = certificate.expect(0x30)?;
        let mut certificate = DerReader::new(certificate);
        let tbs = certificate.expect(0x30)?;
        let mut tbs = DerReader::new(tbs);
        if tbs.peek_tag() == Some(0xa0) {
            let _ = tbs.read_tlv()?;
        }
        let _serial = tbs.read_tlv()?;
        let _signature = tbs.read_tlv()?;
        let _issuer = tbs.read_tlv()?;
        let validity = tbs.expect(0x30)?;
        let mut validity = DerReader::new(validity);
        let _not_before = validity.read_tlv()?;
        let (tag, not_after) = validity.read_tlv()?;
        Self::parse_asn1_time(tag, not_after)
    }

    async fn fetch_tls_certificate_not_after(&self) -> Result<Option<i64>, String> {
        let url = url::Url::parse(&self.base_url).map_err(|error| error.to_string())?;
        if url.scheme() != "https" {
            return Ok(None);
        }
        let host = url
            .host_str()
            .ok_or_else(|| "HTTPS URLにホストがありません".to_string())?
            .to_owned();
        let port = url.port_or_known_default().unwrap_or(443);
        let stream = TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|error| error.to_string())?;
        let server_name = ServerName::try_from(host)
            .map_err(|_| "TLS ServerNameを作成できません".to_string())?;
        let config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(CertificateOnlyVerifier))
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(config));
        let tls = connector
            .connect(server_name, stream)
            .await
            .map_err(|error| error.to_string())?;
        let (_, session) = tls.get_ref();
        let certificate = session
            .peer_certificates()
            .and_then(|certificates| certificates.first())
            .ok_or_else(|| "TLS証明書が提示されませんでした".to_string())?;
        Self::parse_certificate_not_after_ms(certificate.as_ref())
            .map(Some)
            .map_err(|error| error.to_string())
    }

    async fn fetch_voting_key_snapshot(&self) -> Result<CertificateSnapshot, String> {
        let info = self.get_json("/node/info").await.map_err(|error| error.to_string())?;
        let roles = u64_value(info.get("roles")).unwrap_or_default();
        if roles & 4 == 0 {
            return Ok(CertificateSnapshot::default());
        }
        let public_key = info
            .get("publicKey")
            .and_then(string_value)
            .ok_or_else(|| "node/infoにpublicKeyがありません".to_string())?;
        let chain = self
            .get_json("/chain/info")
            .await
            .map_err(|error| error.to_string())?;
        let finalization_epoch = chain
            .get("latestFinalizedBlock")
            .and_then(|block| Self::i64_value(block.get("finalizationEpoch")));
        let accounts = self
            .fetch_accounts_by_public_keys(&[public_key.to_ascii_uppercase()], &mut Vec::new())
            .await;
        let account = accounts
            .first()
            .ok_or_else(|| "投票キー確認用アカウント情報を取得できません".to_string())?;
        let ranges = Self::voting_key_ranges(account);
        let selected = finalization_epoch
            .and_then(|epoch| {
                ranges
                    .iter()
                    .copied()
                    .filter(|(start, end)| *start <= epoch && epoch <= *end)
                    .max_by_key(|(_, end)| *end)
            })
            .or_else(|| ranges.iter().copied().max_by_key(|(_, end)| *end));
        let Some((start, end)) = selected else {
            return Err("voting keyの有効epochを取得できません".into());
        };
        Ok(CertificateSnapshot {
            voting_key_start_epoch: Some(start),
            voting_key_end_epoch: Some(end),
            finalization_epoch,
            voting_key_remaining_epochs: finalization_epoch.map(|epoch| end.saturating_sub(epoch)),
            ..CertificateSnapshot::default()
        })
    }

    fn remaining_days(not_after: i64, now: i64) -> i64 {
        let remaining = not_after.saturating_sub(now);
        let day = 86_400_000;
        if remaining >= 0 {
            (remaining + day - 1) / day
        } else {
            remaining.div_euclid(day)
        }
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

    async fn fetch_certificate_metrics(&self) -> AppResult<CertificateSnapshot> {
        let mut snapshot = CertificateSnapshot::default();
        let now = chrono::Utc::now().timestamp_millis();
        let mut errors = Vec::new();

        match self.fetch_tls_certificate_not_after().await {
            Ok(Some(not_after)) => {
                snapshot.node_certificate_not_after = Some(not_after);
                snapshot.node_certificate_remaining_days = Some(Self::remaining_days(not_after, now));
            }
            Ok(None) => {}
            Err(error) => errors.push(format!("node certificate: {error}")),
        }

        match self.fetch_voting_key_snapshot().await {
            Ok(voting) => {
                snapshot.voting_key_start_epoch = voting.voting_key_start_epoch;
                snapshot.voting_key_end_epoch = voting.voting_key_end_epoch;
                snapshot.finalization_epoch = voting.finalization_epoch;
                snapshot.voting_key_remaining_epochs = voting.voting_key_remaining_epochs;
            }
            Err(error) => errors.push(format!("voting key: {error}")),
        }

        snapshot.error = (!errors.is_empty()).then(|| errors.join("; "));
        Ok(snapshot)
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

    #[test]
    fn extracts_voting_key_epoch_ranges() {
        let account = serde_json::json!({
            "supplementalPublicKeys": {
                "voting": {
                    "publicKeys": [
                        { "publicKey": "A", "startEpoch": 10, "endEpoch": 20 },
                        { "publicKey": "B", "startEpoch": "21", "endEpoch": "40" }
                    ]
                }
            }
        });

        assert_eq!(
            SymbolAdapter::voting_key_ranges(&account),
            vec![(10, 20), (21, 40)]
        );
    }

    #[test]
    fn parses_certificate_asn1_times() {
        assert_eq!(
            SymbolAdapter::parse_asn1_time(0x17, b"260710010203Z").unwrap(),
            1_783_645_323_000
        );
        assert_eq!(
            SymbolAdapter::parse_asn1_time(0x18, b"20560710010203Z").unwrap(),
            2_730_416_523_000
        );
    }

    #[test]
    fn remaining_certificate_days_rounds_expired_values_down() {
        assert_eq!(SymbolAdapter::remaining_days(1_001, 1_000), 1);
        assert_eq!(SymbolAdapter::remaining_days(999, 1_000), -1);
    }
}

mod nis1;
mod symbol;

use reqwest::Client;
use std::time::Duration;
use tokio::net::{lookup_host, TcpStream};
use url::Url;

use crate::{
    domain::{NodeAdapter, NodeKind},
    error::{AppError, AppResult},
};

pub use nis1::Nis1Adapter;
pub use symbol::SymbolAdapter;

pub fn normalize_base_url(input: &str) -> AppResult<String> {
    let mut url = Url::parse(input.trim())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::InvalidUrl(
            "HTTPまたはHTTPSだけを使用できます".into(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::InvalidUrl(
            "URLに資格情報を含めることはできません".into(),
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(AppError::InvalidUrl(
            "queryとfragmentは使用できません".into(),
        ));
    }
    if url.host_str().is_none() {
        return Err(AppError::InvalidUrl("ホスト名がありません".into()));
    }
    url.set_path("");
    let normalized = url.as_str().trim_end_matches('/').to_string();
    Ok(normalized)
}

pub fn normalize_peer_endpoint(input: &str) -> AppResult<String> {
    let raw = input.trim();
    let raw = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("tcp://{raw}")
    };
    let mut url = Url::parse(&raw)?;
    if !matches!(url.scheme(), "tcp" | "symbol-peer") {
        return Err(AppError::InvalidUrl(
            "Peerノードはtcpまたはsymbol-peerだけを使用できます".into(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::InvalidUrl(
            "URLに資格情報を含めることはできません".into(),
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(AppError::InvalidUrl(
            "queryとfragmentは使用できません".into(),
        ));
    }
    if url.host_str().is_none() {
        return Err(AppError::InvalidUrl("ホスト名がありません".into()));
    }
    url.set_scheme("tcp")
        .map_err(|_| AppError::InvalidUrl("Peer URLを正規化できません".into()))?;
    url.set_path("");
    if url.port().is_none() {
        url.set_port(Some(7900))
            .map_err(|_| AppError::InvalidUrl("Peerポートを設定できません".into()))?;
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

pub fn endpoint_host_port(endpoint: &str) -> AppResult<(String, u16)> {
    let url = Url::parse(endpoint)?;
    let host = url
        .host_str()
        .ok_or_else(|| AppError::InvalidUrl("ホスト名がありません".into()))?
        .to_owned();
    let port = url
        .port()
        .or_else(|| matches!(url.scheme(), "tcp" | "symbol-peer").then_some(7900))
        .or_else(|| url.port_or_known_default())
        .ok_or_else(|| AppError::InvalidUrl("ポートがありません".into()))?;
    Ok((host, port))
}

pub async fn probe_tcp_endpoint(endpoint: &str, timeout: Duration) -> AppResult<()> {
    let (host, port) = endpoint_host_port(endpoint)?;
    let mut addresses = tokio::time::timeout(timeout, lookup_host((host.as_str(), port)))
        .await
        .map_err(|_| AppError::Network("名前解決がタイムアウトしました".into()))??;
    let address = addresses
        .next()
        .ok_or_else(|| AppError::Network("接続先アドレスがありません".into()))?;
    tokio::time::timeout(timeout, TcpStream::connect(address))
        .await
        .map_err(|_| AppError::Network("TCP接続がタイムアウトしました".into()))??;
    Ok(())
}

pub fn adapter_for(kind: NodeKind, base_url: String, client: Client) -> Box<dyn NodeAdapter> {
    match kind {
        NodeKind::Symbol => Box::new(SymbolAdapter::new(base_url, client)),
        NodeKind::Nis1 => Box::new(Nis1Adapter::new(base_url, client)),
    }
}

pub async fn probe_endpoint(
    base_url: &str,
    client: Client,
) -> AppResult<crate::domain::NodeIdentity> {
    let base_url = normalize_base_url(base_url)?;
    let symbol = SymbolAdapter::new(base_url.clone(), client.clone());
    let symbol_error = match symbol.probe().await {
        Ok(identity) => return Ok(identity),
        Err(error) => error,
    };
    let nis1 = Nis1Adapter::new(base_url, client);
    let nis1_error = match nis1.probe().await {
        Ok(identity) => return Ok(identity),
        Err(error) => error,
    };
    Err(AppError::UnknownNode {
        symbol: symbol_error.to_string(),
        nis1: nis1_error.to_string(),
    })
}

pub(crate) fn string_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

pub(crate) fn u64_value(value: Option<&serde_json::Value>) -> Option<u64> {
    value.and_then(|value| match value {
        serde_json::Value::String(value) => value.parse().ok(),
        serde_json::Value::Number(value) => value.as_u64(),
        _ => None,
    })
}

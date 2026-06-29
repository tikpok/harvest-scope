use std::{
    net::{IpAddr, Ipv4Addr},
    sync::atomic::Ordering,
    time::Instant,
};

use futures_util::{stream, StreamExt};
use ipnet::Ipv4Net;
use tauri::{AppHandle, Emitter};

use crate::{
    adapters::probe_endpoint,
    domain::{NetworkInterfaceInfo, ScanCandidate, ScanProgress, ScanRequest},
    error::{AppError, AppResult},
    state::AppState,
};

pub fn network_interfaces() -> Vec<NetworkInterfaceInfo> {
    let mut interfaces = local_ip_address::list_afinet_netifas()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(name, address)| match address {
            IpAddr::V4(address) if !address.is_loopback() && !address.is_link_local() => {
                let octets = address.octets();
                Some(NetworkInterfaceInfo {
                    name,
                    address: address.to_string(),
                    cidr: Ipv4Addr::new(octets[0], octets[1], octets[2], 0).to_string() + "/24",
                })
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    interfaces.sort_by(|a, b| a.name.cmp(&b.name));
    interfaces.dedup_by(|a, b| a.cidr == b.cidr);
    interfaces
}

fn allowed_network(network: &Ipv4Net) -> bool {
    let address = network.network();
    address.is_private()
        || (address.octets()[0] == 100 && (64..=127).contains(&address.octets()[1]))
}

pub async fn scan(
    app: AppHandle,
    state: AppState,
    request: ScanRequest,
) -> AppResult<Vec<ScanCandidate>> {
    let network: Ipv4Net = request
        .cidr
        .parse()
        .map_err(|_| AppError::Configuration("CIDRが正しくありません".into()))?;
    if !allowed_network(&network) {
        return Err(AppError::Configuration(
            "RFC1918またはTailscaleのプライベートCIDRだけをスキャンできます".into(),
        ));
    }
    let hosts: Vec<_> = network.hosts().take(1025).collect();
    if hosts.len() > 1024 {
        return Err(AppError::Configuration(
            "1回にスキャンできるのは1,024アドレスまでです".into(),
        ));
    }
    let mut ports = request.ports;
    ports.sort_unstable();
    ports.dedup();
    if ports.is_empty() || ports.len() > 16 || ports.contains(&0) {
        return Err(AppError::Configuration(
            "ポートは1～16個指定してください".into(),
        ));
    }
    state.scan_cancelled.store(false, Ordering::Relaxed);
    let targets = hosts
        .into_iter()
        .flat_map(|host| ports.iter().copied().map(move |port| (host, port)))
        .collect::<Vec<_>>();
    let total = targets.len();
    let state_for_stream = state.clone();
    let app_for_stream = app.clone();
    let results = stream::iter(targets.into_iter().enumerate().map(
        move |(index, (host, port))| {
            let state = state_for_stream.clone();
            let app = app_for_stream.clone();
            async move {
                if state.scan_cancelled.load(Ordering::Relaxed) {
                    return None;
                }
                let scheme = if port == 3001 { "https" } else { "http" };
                let base_url = format!("{scheme}://{host}:{port}");
                let started = Instant::now();
                let identity = probe_endpoint(&base_url, state.client.clone()).await.ok();
                let _ = app.emit(
                    "scan-progress",
                    ScanProgress {
                        scanned: index + 1,
                        total,
                        found: usize::from(identity.is_some()),
                    },
                );
                identity.map(|identity| ScanCandidate {
                    base_url,
                    kind: identity.kind,
                    name: identity.name,
                    network: identity.network,
                    version: identity.version,
                    latency_ms: started.elapsed().as_millis() as i64,
                })
            }
        },
    ))
    .buffer_unordered(64)
    .filter_map(|item| async move { item })
    .collect::<Vec<_>>()
    .await;
    if state.scan_cancelled.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }
    let mut results = results;
    results.sort_by(|a, b| a.base_url.cmp(&b.base_url));
    Ok(results)
}

use std::{
    net::IpAddr,
    time::{Duration, Instant},
};

use tokio::net::{lookup_host, TcpStream};

use crate::{
    adapters::adapter_for,
    domain::{DiagnosticCheck, DiagnosticRun, DiagnosticStatus, NodeKind, StoredNode},
    error::{AppError, AppResult},
    state::AppState,
};

const CHECK_TIMEOUT: Duration = Duration::from_secs(5);
const RUN_TIMEOUT: Duration = Duration::from_secs(30);

fn check(
    key: &str,
    label: &str,
    status: &str,
    message: impl Into<String>,
    recommendation: Option<&str>,
    started: Instant,
) -> DiagnosticCheck {
    DiagnosticCheck {
        key: key.into(),
        label: label.into(),
        status: status.into(),
        message: message.into(),
        recommendation: recommendation.map(str::to_owned),
        duration_ms: started.elapsed().as_millis() as i64,
    }
}

fn summarize(checks: &[DiagnosticCheck]) -> (DiagnosticStatus, String) {
    let failed = checks.iter().filter(|item| item.status == "fail").count();
    let warnings = checks
        .iter()
        .filter(|item| item.status == "warning")
        .count();
    if failed > 0 {
        (
            DiagnosticStatus::Failed,
            format!("{failed}件の異常と{warnings}件の注意事項を検出しました。"),
        )
    } else if warnings > 0 {
        (
            DiagnosticStatus::Warning,
            format!("異常はありませんが、{warnings}件の注意事項があります。"),
        )
    } else {
        (
            DiagnosticStatus::Passed,
            "すべての診断項目が正常です。".into(),
        )
    }
}

async fn execute(state: &AppState, node: &StoredNode, alert_id: Option<String>) -> DiagnosticRun {
    let started_at = chrono::Utc::now().timestamp_millis();
    let mut checks = Vec::new();
    let parsed = url::Url::parse(&node.base_url);
    let (host, port) = match parsed {
        Ok(url) => (
            url.host_str().map(str::to_owned),
            url.port_or_known_default(),
        ),
        Err(error) => {
            checks.push(check(
                "dns",
                "名前解決",
                "fail",
                error.to_string(),
                Some("登録URLを確認してください。"),
                Instant::now(),
            ));
            let (status, summary) = summarize(&checks);
            return DiagnosticRun {
                id: uuid::Uuid::new_v4().to_string(),
                node_id: node.id.clone(),
                alert_id,
                started_at,
                completed_at: chrono::Utc::now().timestamp_millis(),
                status,
                summary,
                checks,
            };
        }
    };

    let address = match (host, port) {
        (Some(host), Some(port)) => {
            let started = Instant::now();
            let resolved = if host.parse::<IpAddr>().is_ok() {
                lookup_host((host.as_str(), port)).await
            } else {
                tokio::time::timeout(CHECK_TIMEOUT, lookup_host((host.as_str(), port)))
                    .await
                    .map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "名前解決がタイムアウトしました",
                        )
                    })
                    .and_then(|result| result)
            };
            match resolved {
                Ok(mut addresses) => {
                    let first = addresses.next();
                    checks.push(check(
                        "dns",
                        "名前解決",
                        if first.is_some() { "pass" } else { "fail" },
                        first.map_or_else(
                            || "接続先アドレスがありません。".into(),
                            |value| value.to_string(),
                        ),
                        first
                            .is_none()
                            .then_some("DNS設定または登録ホスト名を確認してください。"),
                        started,
                    ));
                    first
                }
                Err(error) => {
                    checks.push(check(
                        "dns",
                        "名前解決",
                        "fail",
                        error.to_string(),
                        Some("DNS設定または登録ホスト名を確認してください。"),
                        started,
                    ));
                    None
                }
            }
        }
        _ => {
            checks.push(check(
                "dns",
                "名前解決",
                "fail",
                "ホストまたはポートを特定できません。",
                Some("登録URLにホストとポートを指定してください。"),
                Instant::now(),
            ));
            None
        }
    };

    let started = Instant::now();
    if let Some(address) = address {
        match tokio::time::timeout(CHECK_TIMEOUT, TcpStream::connect(address)).await {
            Ok(Ok(_)) => checks.push(check(
                "tcp",
                "RESTポート",
                "pass",
                format!("{address}へ接続できました。"),
                None,
                started,
            )),
            Ok(Err(error)) => checks.push(check(
                "tcp",
                "RESTポート",
                "fail",
                error.to_string(),
                Some("ノードプロセス、待受ポート、ファイアウォールを確認してください。"),
                started,
            )),
            Err(_) => checks.push(check(
                "tcp",
                "RESTポート",
                "fail",
                "TCP接続がタイムアウトしました。",
                Some("ネットワーク経路とファイアウォールを確認してください。"),
                started,
            )),
        }
    } else {
        checks.push(check(
            "tcp",
            "RESTポート",
            "skipped",
            "接続先を解決できないため省略しました。",
            None,
            started,
        ));
    }

    let adapter = adapter_for(node.kind, node.base_url.clone(), state.client.clone());
    let started = Instant::now();
    match tokio::time::timeout(CHECK_TIMEOUT, adapter.probe()).await {
        Ok(Ok(identity)) => {
            let key_changed = node.public_key.is_some()
                && identity.public_key.is_some()
                && node.public_key != identity.public_key;
            let version_changed = node.version.is_some()
                && identity.version.is_some()
                && node.version != identity.version;
            let status = if key_changed {
                "fail"
            } else if version_changed {
                "warning"
            } else {
                "pass"
            };
            let message = if key_changed {
                "登録時と公開鍵が異なります。".into()
            } else if version_changed {
                format!(
                    "バージョンが {} から {} に変わっています。",
                    node.version.as_deref().unwrap_or("不明"),
                    identity.version.as_deref().unwrap_or("不明")
                )
            } else {
                format!("{}ノードとして確認できました。", node.kind.as_str())
            };
            checks.push(check(
                "identity",
                "ノード識別",
                status,
                message,
                key_changed.then_some("接続先が意図したノードか直ちに確認してください。"),
                started,
            ));
        }
        Ok(Err(error)) => checks.push(check(
            "identity",
            "ノード識別",
            "fail",
            error.to_string(),
            Some("REST APIの公開状態と応答内容を確認してください。"),
            started,
        )),
        Err(_) => checks.push(check(
            "identity",
            "ノード識別",
            "fail",
            "識別情報の取得がタイムアウトしました。",
            Some("REST APIの負荷とネットワークを確認してください。"),
            started,
        )),
    }

    let started = Instant::now();
    match tokio::time::timeout(CHECK_TIMEOUT, adapter.fetch_health()).await {
        Ok(Ok(health)) => checks.push(check(
            "health",
            "ヘルス",
            if health.healthy { "pass" } else { "fail" },
            if health.healthy {
                "REST APIは正常です。"
            } else {
                "REST APIが異常状態を返しました。"
            },
            (!health.healthy).then_some("APIまたはデータベースの状態を確認してください。"),
            started,
        )),
        Ok(Err(error)) => checks.push(check(
            "health",
            "ヘルス",
            "fail",
            error.to_string(),
            Some("ノードのヘルスエンドポイントを確認してください。"),
            started,
        )),
        Err(_) => checks.push(check(
            "health",
            "ヘルス",
            "fail",
            "ヘルス確認がタイムアウトしました。",
            Some("REST APIの負荷を確認してください。"),
            started,
        )),
    }

    let started = Instant::now();
    let chain = match tokio::time::timeout(CHECK_TIMEOUT, adapter.fetch_chain()).await {
        Ok(Ok(chain)) => {
            let comparable_ids = state
                .repository
                .list_stored_nodes(true)
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|candidate| {
                    candidate.kind == node.kind && candidate.network == node.network
                })
                .map(|candidate| candidate.id)
                .collect::<std::collections::HashSet<_>>();
            let monitored_max = {
                let runtime = state.runtime.lock().await;
                runtime
                    .iter()
                    .filter(|(id, _)| comparable_ids.contains(*id))
                    .filter_map(|(_, item)| item.chain.height)
                    .max()
            };
            let network_height = chain.network_height.into_iter().chain(monitored_max).max();
            let lag = network_height
                .zip(chain.height)
                .map(|(network, local)| network.saturating_sub(local));
            let warning_limit = state.settings.read().await.lag_warning_blocks;
            let warning = lag.is_some_and(|value| value >= warning_limit);
            checks.push(check(
                "chain",
                "チェーン同期",
                if warning { "warning" } else { "pass" },
                match (chain.height, lag) {
                    (Some(height), Some(lag)) => {
                        format!("チェーン高 {height}、監視基準から{lag}ブロック差です。")
                    }
                    (Some(height), None) => format!("チェーン高 {height}を取得しました。"),
                    _ => "チェーン高を取得できませんでした。".into(),
                },
                warning.then_some("同期状態、ピア接続、ノード負荷を確認してください。"),
                started,
            ));
            Some(chain)
        }
        Ok(Err(error)) => {
            checks.push(check(
                "chain",
                "チェーン同期",
                "fail",
                error.to_string(),
                Some("チェーンAPIとノード同期状態を確認してください。"),
                started,
            ));
            None
        }
        Err(_) => {
            checks.push(check(
                "chain",
                "チェーン同期",
                "fail",
                "チェーン確認がタイムアウトしました。",
                Some("REST APIの負荷を確認してください。"),
                started,
            ));
            None
        }
    };

    let started = Instant::now();
    match tokio::time::timeout(CHECK_TIMEOUT, adapter.fetch_peers()).await {
        Ok(Ok(peers)) => {
            let active = peers.iter().filter(|peer| peer.status == "active").count();
            checks.push(check(
                "peers",
                "ピア接続",
                if active == 0 { "warning" } else { "pass" },
                format!("アクティブピアは{active}件です。"),
                (active == 0).then_some("ピア設定、P2Pポート、外向き通信を確認してください。"),
                started,
            ));
        }
        Ok(Err(error)) => checks.push(check(
            "peers",
            "ピア接続",
            "warning",
            error.to_string(),
            Some("ピアAPIの公開状態を確認してください。"),
            started,
        )),
        Err(_) => checks.push(check(
            "peers",
            "ピア接続",
            "warning",
            "ピア確認がタイムアウトしました。",
            Some("REST APIの負荷を確認してください。"),
            started,
        )),
    }

    if node.kind == NodeKind::Symbol {
        let started = Instant::now();
        if let Some(spec) = adapter.websocket_capability() {
            match tokio::time::timeout(CHECK_TIMEOUT, tokio_tungstenite::connect_async(&spec.url))
                .await
            {
                Ok(Ok((mut stream, _))) => {
                    let _ = stream.close(None).await;
                    checks.push(check(
                        "websocket",
                        "WebSocket",
                        "pass",
                        "WebSocketへ接続できました。",
                        None,
                        started,
                    ));
                }
                Ok(Err(error)) => checks.push(check(
                    "websocket",
                    "WebSocket",
                    "warning",
                    error.to_string(),
                    Some("WebSocketの経路とリバースプロキシ設定を確認してください。"),
                    started,
                )),
                Err(_) => checks.push(check(
                    "websocket",
                    "WebSocket",
                    "warning",
                    "WebSocket接続がタイムアウトしました。",
                    Some("WebSocketの経路を確認してください。"),
                    started,
                )),
            }
        }

        let started = Instant::now();
        let comparable = state
            .repository
            .list_stored_nodes(true)
            .await
            .unwrap_or_default()
            .into_iter()
            .find(|candidate| {
                candidate.id != node.id
                    && candidate.kind == NodeKind::Symbol
                    && candidate.network == node.network
            });
        if let (Some(comparable), Some(target_height)) = (
            comparable,
            chain.as_ref().and_then(|item| item.finalized_height),
        ) {
            let other = adapter_for(comparable.kind, comparable.base_url, state.client.clone());
            let hashes = tokio::time::timeout(CHECK_TIMEOUT, async {
                let other_height = other
                    .fetch_chain()
                    .await?
                    .finalized_height
                    .unwrap_or(target_height);
                let height = target_height.min(other_height);
                let left = adapter.fetch_state_hash(height).await?;
                let right = other.fetch_state_hash(height).await?;
                Ok::<_, AppError>((height, left, right))
            })
            .await;
            match hashes {
                Ok(Ok((height, Some(left), Some(right)))) => checks.push(check(
                    "state_hash",
                    "State hash",
                    if left == right { "pass" } else { "fail" },
                    if left == right {
                        format!("高{height}のstate hashは一致しています。")
                    } else {
                        format!("高{height}のstate hashが他ノードと一致しません。")
                    },
                    (left != right)
                        .then_some("チェーン分岐やデータ破損の可能性を確認してください。"),
                    started,
                )),
                Ok(Ok(_)) => checks.push(check(
                    "state_hash",
                    "State hash",
                    "skipped",
                    "比較用state hashを取得できませんでした。",
                    None,
                    started,
                )),
                Ok(Err(error)) => checks.push(check(
                    "state_hash",
                    "State hash",
                    "warning",
                    error.to_string(),
                    Some("ブロックAPIの公開状態を確認してください。"),
                    started,
                )),
                Err(_) => checks.push(check(
                    "state_hash",
                    "State hash",
                    "warning",
                    "State hash比較がタイムアウトしました。",
                    Some("REST APIの負荷を確認してください。"),
                    started,
                )),
            }
        } else {
            checks.push(check(
                "state_hash",
                "State hash",
                "skipped",
                "比較可能な同一ネットワークノードがありません。",
                None,
                started,
            ));
        }
    } else {
        checks.push(check(
            "websocket",
            "WebSocket",
            "skipped",
            "NIS1は対象外です。",
            None,
            Instant::now(),
        ));
        checks.push(check(
            "state_hash",
            "State hash",
            "skipped",
            "NIS1は対象外です。",
            None,
            Instant::now(),
        ));
    }

    let (status, summary) = summarize(&checks);
    DiagnosticRun {
        id: uuid::Uuid::new_v4().to_string(),
        node_id: node.id.clone(),
        alert_id,
        started_at,
        completed_at: chrono::Utc::now().timestamp_millis(),
        status,
        summary,
        checks,
    }
}

pub async fn run(
    state: AppState,
    node_id: String,
    alert_id: Option<String>,
) -> AppResult<DiagnosticRun> {
    let node = state.repository.get_stored_node(&node_id).await?;
    if let Some(id) = alert_id.as_deref() {
        let alert = state.repository.get_alert(id).await?;
        if alert.node_id != node_id {
            return Err(AppError::Configuration("障害とノードが一致しません".into()));
        }
    }
    {
        let mut active = state.diagnostic_nodes.lock().await;
        if !active.insert(node_id.clone()) {
            return Err(AppError::Configuration("このノードは診断実行中です".into()));
        }
    }
    let _permit = state
        .request_limit
        .clone()
        .acquire_owned()
        .await
        .map_err(|error| AppError::Configuration(error.to_string()))?;
    let result =
        match tokio::time::timeout(RUN_TIMEOUT, execute(&state, &node, alert_id.clone())).await {
            Ok(run) => run,
            Err(_) => DiagnosticRun {
                id: uuid::Uuid::new_v4().to_string(),
                node_id: node_id.clone(),
                alert_id,
                started_at: chrono::Utc::now().timestamp_millis() - RUN_TIMEOUT.as_millis() as i64,
                completed_at: chrono::Utc::now().timestamp_millis(),
                status: DiagnosticStatus::Failed,
                summary: "診断が30秒以内に完了しませんでした。".into(),
                checks: vec![check(
                    "timeout",
                    "診断時間",
                    "fail",
                    "診断全体がタイムアウトしました。",
                    Some("ネットワーク状態とノード負荷を確認してください。"),
                    Instant::now(),
                )],
            },
        };
    state.diagnostic_nodes.lock().await.remove(&node_id);
    state.repository.save_diagnostic_run(&result).await?;
    Ok(result)
}

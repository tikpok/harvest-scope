use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use crate::{
    error::{AppError, AppResult},
    state::AppState,
};

fn sanitize_untrusted_csv_cell(value: impl AsRef<str>) -> String {
    let value = value.as_ref();
    let significant = value.trim_start_matches(' ');
    if significant.starts_with(['=', '+', '-', '@', '\t', '\r', '\n']) {
        format!("'{value}")
    } else {
        value.to_owned()
    }
}

fn safe_numeric_csv_cell(value: impl AsRef<str>) -> String {
    let value = value.as_ref();
    let unsigned = value.strip_prefix('-').unwrap_or(value);
    let mut parts = unsigned.split('.');
    let integer = parts.next().unwrap_or_default();
    let fraction = parts.next();
    let is_numeric = !integer.is_empty()
        && integer.bytes().all(|byte| byte.is_ascii_digit())
        && fraction
            .is_none_or(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && parts.next().is_none();
    if is_numeric {
        value.to_owned()
    } else {
        sanitize_untrusted_csv_cell(value)
    }
}

fn csv_bytes(records: Vec<Vec<String>>) -> AppResult<Vec<u8>> {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    {
        let mut writer = csv::Writer::from_writer(&mut bytes);
        for record in records {
            writer.write_record(record)?;
        }
        writer.flush()?;
    }
    Ok(bytes)
}

pub async fn export(
    app: AppHandle,
    state: &AppState,
    dataset: String,
    node_id: Option<String>,
) -> AppResult<Option<String>> {
    let records = match dataset.as_str() {
        "nodes" => {
            let mut rows = vec![vec![
                "名前".into(),
                "種別".into(),
                "URL".into(),
                "ネットワーク".into(),
                "状態".into(),
                "高さ".into(),
                "遅延".into(),
                "応答ms".into(),
                "委任者数".into(),
                "24時間増減".into(),
                "合計XYM(micro)".into(),
                "ノード証明書期限UTCms".into(),
                "ノード証明書残日数".into(),
                "投票キー終了epoch".into(),
                "投票キー残epoch".into(),
                "証明書取得エラー".into(),
                "Supernode ID".into(),
                "Supernode状態".into(),
                "Supernode最新ラウンド".into(),
                "Supernode最終確認UTCms".into(),
                "Supernode合格".into(),
                "Supernode失敗項目".into(),
                "Supernode取得エラー".into(),
            ]];
            rows.extend(
                state
                    .repository
                    .list_nodes()
                    .await?
                    .into_iter()
                    .map(|node| {
                        vec![
                            sanitize_untrusted_csv_cell(node.display_name),
                            node.kind.as_str().into(),
                            sanitize_untrusted_csv_cell(node.base_url),
                            sanitize_untrusted_csv_cell(node.network.unwrap_or_default()),
                            node.status.as_str().into(),
                            safe_numeric_csv_cell(node.height.unwrap_or_default()),
                            safe_numeric_csv_cell(node.lag.unwrap_or_default()),
                            node.latency_ms
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.delegated_harvester_count
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.delegated_harvester_delta
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            safe_numeric_csv_cell(node.total_balance_micro_xym.unwrap_or_default()),
                            node.node_certificate_not_after
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.node_certificate_remaining_days
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.voting_key_end_epoch
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.voting_key_remaining_epochs
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            sanitize_untrusted_csv_cell(
                                node.certificate_metric_error.unwrap_or_default(),
                            ),
                            node.supernode_id
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            sanitize_untrusted_csv_cell(node.supernode_status.unwrap_or_default()),
                            node.supernode_last_round
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.supernode_last_round_at
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.supernode_checks_passed
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            sanitize_untrusted_csv_cell(node.supernode_failed_checks.join("|")),
                            sanitize_untrusted_csv_cell(
                                node.supernode_metric_error.unwrap_or_default(),
                            ),
                        ]
                    }),
            );
            rows
        }
        "alerts" => {
            let mut rows = vec![vec![
                "ノード".into(),
                "種類".into(),
                "重大度".into(),
                "状態".into(),
                "メッセージ".into(),
                "発生時刻UTCms".into(),
                "更新時刻UTCms".into(),
            ]];
            rows.extend(
                state
                    .repository
                    .list_alerts()
                    .await?
                    .into_iter()
                    .map(|alert| {
                        vec![
                            sanitize_untrusted_csv_cell(alert.node_name),
                            sanitize_untrusted_csv_cell(alert.alert_type),
                            sanitize_untrusted_csv_cell(alert.severity),
                            sanitize_untrusted_csv_cell(alert.state),
                            sanitize_untrusted_csv_cell(alert.message),
                            alert.raised_at.to_string(),
                            alert.updated_at.to_string(),
                        ]
                    }),
            );
            rows
        }
        "metrics" => {
            let id = node_id.ok_or_else(|| {
                AppError::Configuration("メトリクス出力にはnodeIdが必要です".into())
            })?;
            let mut rows = vec![vec![
                "時刻UTCms".into(),
                "到達".into(),
                "高さ".into(),
                "ファイナライズ高".into(),
                "同期遅延".into(),
                "ピア数".into(),
                "応答ms".into(),
                "委任者数".into(),
                "24時間増減".into(),
                "合計XYM(micro)".into(),
                "合計importance".into(),
                "ノード証明書残日数".into(),
                "投票キー残epoch".into(),
                "Supernode合格".into(),
                "Supernode最新ラウンド".into(),
            ]];
            let metrics = state
                .repository
                .query_metrics(&id, 0, chrono::Utc::now().timestamp_millis())
                .await?;
            rows.extend(metrics.into_iter().map(|point| {
                vec![
                    point.observed_at.to_string(),
                    point.reachable.to_string(),
                    safe_numeric_csv_cell(point.height.unwrap_or_default()),
                    safe_numeric_csv_cell(point.finalized_height.unwrap_or_default()),
                    safe_numeric_csv_cell(point.lag.unwrap_or_default()),
                    point
                        .peer_count
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    point
                        .latency_ms
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    point
                        .delegated_harvester_count
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    point
                        .delegated_harvester_delta
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    safe_numeric_csv_cell(point.total_balance_micro_xym.unwrap_or_default()),
                    safe_numeric_csv_cell(point.total_importance.unwrap_or_default()),
                    point
                        .node_certificate_remaining_days
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    point
                        .voting_key_remaining_epochs
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    point
                        .supernode_checks_passed
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    point
                        .supernode_last_round
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                ]
            }));
            rows
        }
        _ => {
            return Err(AppError::Configuration(
                "未対応のCSVデータセットです".into(),
            ))
        }
    };
    let bytes = csv_bytes(records)?;
    let file_name = format!(
        "harvest-scope-{dataset}-{}.csv",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let path = app
        .dialog()
        .file()
        .add_filter("CSV", &["csv"])
        .set_file_name(file_name)
        .blocking_save_file();
    let Some(path) = path else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| AppError::Io(std::io::Error::other(error)))?;
    std::fs::write(&path, bytes)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_csv_cells_neutralize_formula_prefixes_after_leading_spaces() {
        for value in [
            "=1+1",
            "+cmd",
            "-2+3",
            "@SUM(A1)",
            "\tformula",
            "\rformula",
            "  =1+1",
        ] {
            assert_eq!(sanitize_untrusted_csv_cell(value), format!("'{value}"));
        }
    }

    #[test]
    fn untrusted_csv_cells_keep_normal_japanese_and_numeric_cells_stay_numeric() {
        assert_eq!(sanitize_untrusted_csv_cell("正常なノード"), "正常なノード");
        assert_eq!(safe_numeric_csv_cell("-123"), "-123");
        assert_eq!(safe_numeric_csv_cell("=1+1"), "'=1+1");
    }
}

use std::{collections::HashMap, path::Path, str::FromStr, time::Duration};

use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow},
    Row, SqlitePool,
};

use crate::{
    domain::{
        AlertMutation, AlertRecord, AppSettings, DiagnosticRun, DiagnosticStatus, HarvestEvent,
        MetricPoint, MonitorMode, NodeDetail, NodeIdentity, NodeKind, NodeSample, NodeStatus,
        NodeSummary, PeerInfo, StoredNode,
    },
    error::{AppError, AppResult},
};

#[derive(Clone)]
pub struct Repository {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Default)]
pub struct TopologyHealth {
    pub api_status: Option<String>,
    pub db_status: Option<String>,
    pub nis_status_code: Option<i64>,
}

#[derive(Debug, Clone, Default)]
struct HarvestMetricSnapshot {
    delegated_harvester_count: Option<i64>,
    delegated_harvester_delta: Option<i64>,
    total_importance: Option<String>,
    total_balance_micro_xym: Option<String>,
    harvest_metric_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct CertificateMetricSnapshot {
    node_certificate_not_after: Option<i64>,
    node_certificate_remaining_days: Option<i64>,
    voting_key_start_epoch: Option<i64>,
    voting_key_end_epoch: Option<i64>,
    finalization_epoch: Option<i64>,
    voting_key_remaining_epochs: Option<i64>,
    certificate_metric_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct SupernodeMetricSnapshot {
    supernode_id: Option<i64>,
    supernode_status: Option<String>,
    supernode_last_round: Option<i64>,
    supernode_last_round_at: Option<i64>,
    supernode_checks_passed: Option<bool>,
    supernode_failed_checks: Vec<String>,
    supernode_metric_error: Option<String>,
}

fn parse_kind(value: &str) -> AppResult<NodeKind> {
    NodeKind::try_from(value).map_err(AppError::Configuration)
}

fn parse_status(value: &str) -> NodeStatus {
    match value {
        "healthy" => NodeStatus::Healthy,
        "warning" => NodeStatus::Warning,
        "critical" => NodeStatus::Critical,
        "offline" => NodeStatus::Offline,
        _ => NodeStatus::Unknown,
    }
}

fn parse_monitor_mode(value: &str) -> AppResult<MonitorMode> {
    MonitorMode::try_from(value).map_err(AppError::Configuration)
}

fn to_i64(value: Option<u64>) -> Option<i64> {
    value.and_then(|value| i64::try_from(value).ok())
}

fn bool_from_i64(value: Option<i64>) -> Option<bool> {
    value.map(|value| value != 0)
}

fn parse_string_list(value: Option<String>) -> Vec<String> {
    value
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default()
}

fn alert_from_row(row: &SqliteRow) -> AppResult<AlertRecord> {
    Ok(AlertRecord {
        id: row.try_get("id")?,
        node_id: row.try_get("node_id")?,
        node_name: row.try_get("display_name")?,
        alert_type: row.try_get("alert_type")?,
        severity: row.try_get("severity")?,
        state: row.try_get("state")?,
        message: row.try_get("message")?,
        raised_at: row.try_get("raised_at")?,
        updated_at: row.try_get("updated_at")?,
        acknowledged_at: row.try_get("acknowledged_at")?,
        resolved_at: row.try_get("resolved_at")?,
        occurrence_count: row.try_get("occurrence_count")?,
    })
}

fn diagnostic_from_row(row: &SqliteRow) -> AppResult<DiagnosticRun> {
    let status = DiagnosticStatus::try_from(row.try_get::<String, _>("status")?.as_str())
        .map_err(AppError::Configuration)?;
    Ok(DiagnosticRun {
        id: row.try_get("id")?,
        node_id: row.try_get("node_id")?,
        alert_id: row.try_get("alert_id")?,
        started_at: row.try_get("started_at")?,
        completed_at: row.try_get("completed_at")?,
        status,
        summary: row.try_get("summary")?,
        checks: serde_json::from_str(&row.try_get::<String, _>("checks_json")?)?,
    })
}

impl Repository {
    pub async fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let url = format!("sqlite://{}", path.to_string_lossy().replace('\\', "/"));
        let options = SqliteConnectOptions::from_str(&url)
            .map_err(|error| AppError::Configuration(error.to_string()))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        sqlx::query("PRAGMA synchronous = NORMAL")
            .execute(&pool)
            .await?;
        Ok(Self { pool })
    }

    pub async fn list_stored_nodes(&self, enabled_only: bool) -> AppResult<Vec<StoredNode>> {
        let sql = if enabled_only {
            "SELECT * FROM nodes WHERE enabled = 1 ORDER BY sort_order, display_name COLLATE NOCASE, id"
        } else {
            "SELECT * FROM nodes ORDER BY sort_order, display_name COLLATE NOCASE, id"
        };
        let rows = sqlx::query(sql).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(StoredNode {
                    id: row.try_get("id")?,
                    display_name: row.try_get("display_name")?,
                    kind: parse_kind(row.try_get::<String, _>("kind")?.as_str())?,
                    monitor_mode: parse_monitor_mode(
                        row.try_get::<String, _>("monitor_mode")?.as_str(),
                    )?,
                    base_url: row.try_get("base_url")?,
                    network: row.try_get("network")?,
                    version: row.try_get("version")?,
                    public_key: row.try_get("public_key")?,
                    roles: serde_json::from_str(&row.try_get::<String, _>("roles_json")?)
                        .unwrap_or_default(),
                    rest_version: row.try_get("rest_version")?,
                    enabled: row.try_get::<i64, _>("enabled")? != 0,
                    favorite: row.try_get::<i64, _>("favorite")? != 0,
                    show_harvest_metrics: row.try_get::<i64, _>("show_harvest_metrics")? != 0,
                })
            })
            .collect()
    }

    pub async fn get_stored_node(&self, id: &str) -> AppResult<StoredNode> {
        self.list_stored_nodes(false)
            .await?
            .into_iter()
            .find(|node| node.id == id)
            .ok_or_else(|| AppError::NotFound(id.into()))
    }

    pub async fn list_nodes(&self) -> AppResult<Vec<NodeSummary>> {
        let nodes = self.list_stored_nodes(false).await?;
        let mut summaries = Vec::with_capacity(nodes.len());
        for node in &nodes {
            summaries.push(self.node_summary(node).await?);
        }
        Ok(summaries)
    }

    async fn node_summary(&self, node: &StoredNode) -> AppResult<NodeSummary> {
        let sample = sqlx::query(
            "SELECT * FROM node_samples WHERE node_id = ? ORDER BY observed_at DESC LIMIT 1",
        )
        .bind(&node.id)
        .fetch_optional(&self.pool)
        .await?;
        let harvest = if node.show_harvest_metrics {
            self.latest_harvest_metrics(&node.id).await?
        } else {
            HarvestMetricSnapshot::default()
        };
        let latest_harvest_at = if node.show_harvest_metrics {
            self.latest_harvest_at(&node.id).await?
        } else {
            None
        };
        let certificate = self.latest_certificate_metrics(&node.id).await?;
        let supernode = self.latest_supernode_metrics(&node.id).await?;
        let summary = if let Some(row) = sample {
            NodeSummary {
                id: node.id.clone(),
                display_name: node.display_name.clone(),
                kind: node.kind,
                monitor_mode: node.monitor_mode,
                base_url: node.base_url.clone(),
                network: node.network.clone(),
                version: node.version.clone(),
                roles: node.roles.clone(),
                enabled: node.enabled,
                favorite: node.favorite,
                status: if node.enabled {
                    parse_status(row.try_get::<String, _>("status")?.as_str())
                } else {
                    NodeStatus::Unknown
                },
                height: row
                    .try_get::<Option<i64>, _>("height")?
                    .map(|value| value.to_string()),
                finalized_height: row
                    .try_get::<Option<i64>, _>("finalized_height")?
                    .map(|value| value.to_string()),
                lag: row
                    .try_get::<Option<i64>, _>("lag")?
                    .map(|value| value.to_string()),
                peer_count: row.try_get("peer_count")?,
                latency_ms: row.try_get("latency_ms")?,
                last_seen_at: Some(row.try_get("observed_at")?),
                last_error: row.try_get("last_error")?,
                show_harvest_metrics: node.show_harvest_metrics,
                delegated_harvester_count: row
                    .try_get::<Option<i64>, _>("delegated_harvester_count")?
                    .or(harvest.delegated_harvester_count),
                delegated_harvester_delta: row
                    .try_get::<Option<i64>, _>("delegated_harvester_delta")?
                    .or(harvest.delegated_harvester_delta),
                total_importance: row
                    .try_get::<Option<String>, _>("total_importance")?
                    .or(harvest.total_importance),
                total_balance_micro_xym: row
                    .try_get::<Option<String>, _>("total_balance_micro_xym")?
                    .or(harvest.total_balance_micro_xym),
                harvest_metric_error: row
                    .try_get::<Option<String>, _>("harvest_metric_error")?
                    .or(harvest.harvest_metric_error),
                latest_harvest_at,
                node_certificate_not_after: row
                    .try_get::<Option<i64>, _>("node_certificate_not_after")?
                    .or(certificate.node_certificate_not_after),
                node_certificate_remaining_days: row
                    .try_get::<Option<i64>, _>("node_certificate_remaining_days")?
                    .or(certificate.node_certificate_remaining_days),
                voting_key_start_epoch: row
                    .try_get::<Option<i64>, _>("voting_key_start_epoch")?
                    .or(certificate.voting_key_start_epoch),
                voting_key_end_epoch: row
                    .try_get::<Option<i64>, _>("voting_key_end_epoch")?
                    .or(certificate.voting_key_end_epoch),
                finalization_epoch: row
                    .try_get::<Option<i64>, _>("finalization_epoch")?
                    .or(certificate.finalization_epoch),
                voting_key_remaining_epochs: row
                    .try_get::<Option<i64>, _>("voting_key_remaining_epochs")?
                    .or(certificate.voting_key_remaining_epochs),
                certificate_metric_error: row
                    .try_get::<Option<String>, _>("certificate_metric_error")?
                    .or(certificate.certificate_metric_error),
                supernode_id: row
                    .try_get::<Option<i64>, _>("supernode_id")?
                    .or(supernode.supernode_id),
                supernode_status: row
                    .try_get::<Option<String>, _>("supernode_status")?
                    .or(supernode.supernode_status),
                supernode_last_round: row
                    .try_get::<Option<i64>, _>("supernode_last_round")?
                    .or(supernode.supernode_last_round),
                supernode_last_round_at: row
                    .try_get::<Option<i64>, _>("supernode_last_round_at")?
                    .or(supernode.supernode_last_round_at),
                supernode_checks_passed: bool_from_i64(
                    row.try_get::<Option<i64>, _>("supernode_checks_passed")?,
                )
                .or(supernode.supernode_checks_passed),
                supernode_failed_checks: {
                    let failed = parse_string_list(row.try_get("supernode_failed_checks_json")?);
                    if failed.is_empty() {
                        supernode.supernode_failed_checks
                    } else {
                        failed
                    }
                },
                supernode_metric_error: row
                    .try_get::<Option<String>, _>("supernode_metric_error")?
                    .or(supernode.supernode_metric_error),
            }
        } else {
            NodeSummary {
                id: node.id.clone(),
                display_name: node.display_name.clone(),
                kind: node.kind,
                monitor_mode: node.monitor_mode,
                base_url: node.base_url.clone(),
                network: node.network.clone(),
                version: node.version.clone(),
                roles: node.roles.clone(),
                enabled: node.enabled,
                favorite: node.favorite,
                status: NodeStatus::Unknown,
                height: None,
                finalized_height: None,
                lag: None,
                peer_count: None,
                latency_ms: None,
                last_seen_at: None,
                last_error: None,
                show_harvest_metrics: node.show_harvest_metrics,
                delegated_harvester_count: harvest.delegated_harvester_count,
                delegated_harvester_delta: harvest.delegated_harvester_delta,
                total_importance: harvest.total_importance,
                total_balance_micro_xym: harvest.total_balance_micro_xym,
                harvest_metric_error: harvest.harvest_metric_error,
                latest_harvest_at,
                node_certificate_not_after: certificate.node_certificate_not_after,
                node_certificate_remaining_days: certificate.node_certificate_remaining_days,
                voting_key_start_epoch: certificate.voting_key_start_epoch,
                voting_key_end_epoch: certificate.voting_key_end_epoch,
                finalization_epoch: certificate.finalization_epoch,
                voting_key_remaining_epochs: certificate.voting_key_remaining_epochs,
                certificate_metric_error: certificate.certificate_metric_error,
                supernode_id: supernode.supernode_id,
                supernode_status: supernode.supernode_status,
                supernode_last_round: supernode.supernode_last_round,
                supernode_last_round_at: supernode.supernode_last_round_at,
                supernode_checks_passed: supernode.supernode_checks_passed,
                supernode_failed_checks: supernode.supernode_failed_checks,
                supernode_metric_error: supernode.supernode_metric_error,
            }
        };
        Ok(summary)
    }

    #[allow(dead_code)]
    pub async fn upsert_node(
        &self,
        id: &str,
        base_url: &str,
        display_name: &str,
        identity: &NodeIdentity,
        enabled: bool,
        favorite: bool,
    ) -> AppResult<()> {
        let now = chrono::Utc::now().timestamp_millis();
        let roles = serde_json::to_string(&identity.roles)?;
        sqlx::query(
            "INSERT INTO nodes (id, display_name, kind, monitor_mode, base_url, network, version, public_key, roles_json, rest_version, enabled, favorite, sort_order, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, (SELECT COALESCE(MAX(sort_order) + 1, 0) FROM nodes), ?, ?)
             ON CONFLICT(id) DO UPDATE SET display_name=excluded.display_name, kind=excluded.kind, monitor_mode=excluded.monitor_mode, base_url=excluded.base_url,
             network=excluded.network, version=excluded.version, public_key=excluded.public_key, roles_json=excluded.roles_json,
             rest_version=excluded.rest_version, enabled=excluded.enabled, favorite=excluded.favorite, updated_at=excluded.updated_at"
        ).bind(id).bind(display_name).bind(identity.kind.as_str()).bind(MonitorMode::Api.as_str()).bind(base_url).bind(&identity.network).bind(&identity.version)
            .bind(&identity.public_key).bind(roles).bind(&identity.rest_version).bind(enabled as i64).bind(favorite as i64)
            .bind(now).bind(now).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn upsert_node_with_mode(
        &self,
        id: &str,
        base_url: &str,
        display_name: &str,
        identity: &NodeIdentity,
        monitor_mode: MonitorMode,
        enabled: bool,
        favorite: bool,
    ) -> AppResult<()> {
        let now = chrono::Utc::now().timestamp_millis();
        let roles = serde_json::to_string(&identity.roles)?;
        sqlx::query(
            "INSERT INTO nodes (id, display_name, kind, monitor_mode, base_url, network, version, public_key, roles_json, rest_version, enabled, favorite, sort_order, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, (SELECT COALESCE(MAX(sort_order) + 1, 0) FROM nodes), ?, ?)
             ON CONFLICT(id) DO UPDATE SET display_name=excluded.display_name, kind=excluded.kind, monitor_mode=excluded.monitor_mode, base_url=excluded.base_url,
             network=excluded.network, version=excluded.version, public_key=excluded.public_key, roles_json=excluded.roles_json,
             rest_version=excluded.rest_version, enabled=excluded.enabled, favorite=excluded.favorite, updated_at=excluded.updated_at"
        ).bind(id).bind(display_name).bind(identity.kind.as_str()).bind(monitor_mode.as_str()).bind(base_url).bind(&identity.network).bind(&identity.version)
            .bind(&identity.public_key).bind(roles).bind(&identity.rest_version).bind(enabled as i64).bind(favorite as i64)
            .bind(now).bind(now).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> AppResult<()> {
        let result = sqlx::query("UPDATE nodes SET enabled = ?, updated_at = ? WHERE id = ?")
            .bind(enabled as i64)
            .bind(chrono::Utc::now().timestamp_millis())
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(id.into()));
        }
        Ok(())
    }

    pub async fn set_harvest_metrics_visible(&self, id: &str, visible: bool) -> AppResult<()> {
        let result =
            sqlx::query("UPDATE nodes SET show_harvest_metrics = ?, updated_at = ? WHERE id = ?")
                .bind(visible as i64)
                .bind(chrono::Utc::now().timestamp_millis())
                .bind(id)
                .execute(&self.pool)
                .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(id.into()));
        }
        Ok(())
    }

    pub async fn delete_node(&self, id: &str) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM nodes WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(id.into()));
        }
        Ok(())
    }

    pub async fn move_node(&self, id: &str, offset: isize) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query(
            "SELECT id FROM nodes ORDER BY sort_order, display_name COLLATE NOCASE, id",
        )
        .fetch_all(&mut *tx)
        .await?;
        let mut ids = rows
            .into_iter()
            .map(|row| row.try_get::<String, _>("id"))
            .collect::<Result<Vec<_>, _>>()?;
        let current = ids
            .iter()
            .position(|node_id| node_id == id)
            .ok_or_else(|| AppError::NotFound(id.into()))?;
        let target = current.checked_add_signed(offset);
        let Some(target) = target.filter(|target| *target < ids.len()) else {
            return Ok(());
        };
        ids.swap(current, target);
        let now = chrono::Utc::now().timestamp_millis();
        for (sort_order, node_id) in ids.iter().enumerate() {
            sqlx::query("UPDATE nodes SET sort_order = ?, updated_at = ? WHERE id = ?")
                .bind(sort_order as i64)
                .bind(now)
                .bind(node_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn save_sample(&self, node_id: &str, sample: &NodeSample) -> AppResult<()> {
        let supernode_failed_checks = serde_json::to_string(&sample.supernode_failed_checks)?;
        sqlx::query(
            "INSERT INTO node_samples (node_id, observed_at, reachable, status, latency_ms, height, finalized_height, network_height, lag, peer_count, chain_score, api_status, db_status, nis_status_code, last_error, delegated_harvester_count, delegated_harvester_delta, total_importance, total_balance_micro_xym, harvest_metric_error, node_certificate_not_after, node_certificate_remaining_days, voting_key_start_epoch, voting_key_end_epoch, finalization_epoch, voting_key_remaining_epochs, certificate_metric_error, supernode_id, supernode_status, supernode_last_round, supernode_last_round_at, supernode_checks_passed, supernode_failed_checks_json, supernode_metric_error)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        ).bind(node_id).bind(sample.observed_at).bind(sample.reachable as i64).bind(sample.status.as_str())
            .bind(sample.latency_ms).bind(to_i64(sample.height)).bind(to_i64(sample.finalized_height))
            .bind(to_i64(sample.network_height)).bind(to_i64(sample.lag)).bind(sample.peer_count)
            .bind(&sample.chain_score).bind(&sample.api_status).bind(&sample.db_status).bind(sample.nis_status_code)
            .bind(&sample.last_error).bind(sample.delegated_harvester_count).bind(sample.delegated_harvester_delta)
            .bind(&sample.total_importance).bind(&sample.total_balance_micro_xym).bind(&sample.harvest_metric_error)
            .bind(sample.node_certificate_not_after).bind(sample.node_certificate_remaining_days)
            .bind(sample.voting_key_start_epoch).bind(sample.voting_key_end_epoch).bind(sample.finalization_epoch)
            .bind(sample.voting_key_remaining_epochs).bind(&sample.certificate_metric_error)
            .bind(sample.supernode_id).bind(&sample.supernode_status).bind(sample.supernode_last_round)
            .bind(sample.supernode_last_round_at).bind(sample.supernode_checks_passed.map(|value| if value { 1 } else { 0 }))
            .bind(supernode_failed_checks).bind(&sample.supernode_metric_error)
            .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn harvester_count_at_or_before(
        &self,
        node_id: &str,
        observed_at: i64,
    ) -> AppResult<Option<i64>> {
        let row = sqlx::query(
            "SELECT delegated_harvester_count FROM node_samples WHERE node_id = ? AND observed_at <= ? AND delegated_harvester_count IS NOT NULL ORDER BY observed_at DESC LIMIT 1",
        )
        .bind(node_id)
        .bind(observed_at)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| row.try_get("delegated_harvester_count"))
            .transpose()
            .map_err(Into::into)
    }

    async fn latest_harvest_metrics(&self, node_id: &str) -> AppResult<HarvestMetricSnapshot> {
        let row = sqlx::query(
            "SELECT delegated_harvester_count, delegated_harvester_delta, total_importance, total_balance_micro_xym, harvest_metric_error
             FROM node_samples
             WHERE node_id = ?
               AND (delegated_harvester_count IS NOT NULL OR total_importance IS NOT NULL OR total_balance_micro_xym IS NOT NULL OR harvest_metric_error IS NOT NULL)
             ORDER BY observed_at DESC
             LIMIT 1",
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(HarvestMetricSnapshot::default());
        };
        Ok(HarvestMetricSnapshot {
            delegated_harvester_count: row.try_get("delegated_harvester_count")?,
            delegated_harvester_delta: row.try_get("delegated_harvester_delta")?,
            total_importance: row.try_get("total_importance")?,
            total_balance_micro_xym: row.try_get("total_balance_micro_xym")?,
            harvest_metric_error: row.try_get("harvest_metric_error")?,
        })
    }

    async fn latest_harvest_metrics_at_or_before(
        &self,
        node_id: &str,
        observed_at: i64,
    ) -> AppResult<HarvestMetricSnapshot> {
        let row = sqlx::query(
            "SELECT delegated_harvester_count, delegated_harvester_delta, total_importance, total_balance_micro_xym, harvest_metric_error
             FROM node_samples
             WHERE node_id = ?
               AND observed_at <= ?
               AND (delegated_harvester_count IS NOT NULL OR total_importance IS NOT NULL OR total_balance_micro_xym IS NOT NULL OR harvest_metric_error IS NOT NULL)
             ORDER BY observed_at DESC
             LIMIT 1",
        )
        .bind(node_id)
        .bind(observed_at)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(HarvestMetricSnapshot::default());
        };
        Ok(HarvestMetricSnapshot {
            delegated_harvester_count: row.try_get("delegated_harvester_count")?,
            delegated_harvester_delta: row.try_get("delegated_harvester_delta")?,
            total_importance: row.try_get("total_importance")?,
            total_balance_micro_xym: row.try_get("total_balance_micro_xym")?,
            harvest_metric_error: row.try_get("harvest_metric_error")?,
        })
    }

    async fn latest_certificate_metrics(
        &self,
        node_id: &str,
    ) -> AppResult<CertificateMetricSnapshot> {
        let row = sqlx::query(
            "SELECT node_certificate_not_after, node_certificate_remaining_days, voting_key_start_epoch, voting_key_end_epoch, finalization_epoch, voting_key_remaining_epochs, certificate_metric_error
             FROM node_samples
             WHERE node_id = ?
               AND (node_certificate_not_after IS NOT NULL OR node_certificate_remaining_days IS NOT NULL OR voting_key_start_epoch IS NOT NULL OR voting_key_end_epoch IS NOT NULL OR finalization_epoch IS NOT NULL OR voting_key_remaining_epochs IS NOT NULL OR certificate_metric_error IS NOT NULL)
             ORDER BY observed_at DESC
             LIMIT 1",
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(CertificateMetricSnapshot::default());
        };
        Ok(CertificateMetricSnapshot {
            node_certificate_not_after: row.try_get("node_certificate_not_after")?,
            node_certificate_remaining_days: row.try_get("node_certificate_remaining_days")?,
            voting_key_start_epoch: row.try_get("voting_key_start_epoch")?,
            voting_key_end_epoch: row.try_get("voting_key_end_epoch")?,
            finalization_epoch: row.try_get("finalization_epoch")?,
            voting_key_remaining_epochs: row.try_get("voting_key_remaining_epochs")?,
            certificate_metric_error: row.try_get("certificate_metric_error")?,
        })
    }

    async fn latest_certificate_metrics_at_or_before(
        &self,
        node_id: &str,
        observed_at: i64,
    ) -> AppResult<CertificateMetricSnapshot> {
        let row = sqlx::query(
            "SELECT node_certificate_not_after, node_certificate_remaining_days, voting_key_start_epoch, voting_key_end_epoch, finalization_epoch, voting_key_remaining_epochs, certificate_metric_error
             FROM node_samples
             WHERE node_id = ?
               AND observed_at <= ?
               AND (node_certificate_not_after IS NOT NULL OR node_certificate_remaining_days IS NOT NULL OR voting_key_start_epoch IS NOT NULL OR voting_key_end_epoch IS NOT NULL OR finalization_epoch IS NOT NULL OR voting_key_remaining_epochs IS NOT NULL OR certificate_metric_error IS NOT NULL)
             ORDER BY observed_at DESC
             LIMIT 1",
        )
        .bind(node_id)
        .bind(observed_at)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(CertificateMetricSnapshot::default());
        };
        Ok(CertificateMetricSnapshot {
            node_certificate_not_after: row.try_get("node_certificate_not_after")?,
            node_certificate_remaining_days: row.try_get("node_certificate_remaining_days")?,
            voting_key_start_epoch: row.try_get("voting_key_start_epoch")?,
            voting_key_end_epoch: row.try_get("voting_key_end_epoch")?,
            finalization_epoch: row.try_get("finalization_epoch")?,
            voting_key_remaining_epochs: row.try_get("voting_key_remaining_epochs")?,
            certificate_metric_error: row.try_get("certificate_metric_error")?,
        })
    }

    async fn latest_supernode_metrics(&self, node_id: &str) -> AppResult<SupernodeMetricSnapshot> {
        let row = sqlx::query(
            "SELECT supernode_id, supernode_status, supernode_last_round, supernode_last_round_at, supernode_checks_passed, supernode_failed_checks_json, supernode_metric_error
             FROM node_samples
             WHERE node_id = ?
               AND (supernode_id IS NOT NULL OR supernode_status IS NOT NULL OR supernode_last_round IS NOT NULL OR supernode_checks_passed IS NOT NULL OR supernode_metric_error IS NOT NULL)
             ORDER BY observed_at DESC
             LIMIT 1",
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(SupernodeMetricSnapshot::default());
        };
        Ok(SupernodeMetricSnapshot {
            supernode_id: row.try_get("supernode_id")?,
            supernode_status: row.try_get("supernode_status")?,
            supernode_last_round: row.try_get("supernode_last_round")?,
            supernode_last_round_at: row.try_get("supernode_last_round_at")?,
            supernode_checks_passed: bool_from_i64(row.try_get("supernode_checks_passed")?),
            supernode_failed_checks: parse_string_list(
                row.try_get("supernode_failed_checks_json")?,
            ),
            supernode_metric_error: row.try_get("supernode_metric_error")?,
        })
    }

    async fn latest_supernode_metrics_at_or_before(
        &self,
        node_id: &str,
        observed_at: i64,
    ) -> AppResult<SupernodeMetricSnapshot> {
        let row = sqlx::query(
            "SELECT supernode_id, supernode_status, supernode_last_round, supernode_last_round_at, supernode_checks_passed, supernode_failed_checks_json, supernode_metric_error
             FROM node_samples
             WHERE node_id = ?
               AND observed_at <= ?
               AND (supernode_id IS NOT NULL OR supernode_status IS NOT NULL OR supernode_last_round IS NOT NULL OR supernode_checks_passed IS NOT NULL OR supernode_metric_error IS NOT NULL)
             ORDER BY observed_at DESC
             LIMIT 1",
        )
        .bind(node_id)
        .bind(observed_at)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(SupernodeMetricSnapshot::default());
        };
        Ok(SupernodeMetricSnapshot {
            supernode_id: row.try_get("supernode_id")?,
            supernode_status: row.try_get("supernode_status")?,
            supernode_last_round: row.try_get("supernode_last_round")?,
            supernode_last_round_at: row.try_get("supernode_last_round_at")?,
            supernode_checks_passed: bool_from_i64(row.try_get("supernode_checks_passed")?),
            supernode_failed_checks: parse_string_list(
                row.try_get("supernode_failed_checks_json")?,
            ),
            supernode_metric_error: row.try_get("supernode_metric_error")?,
        })
    }

    pub async fn replace_harvesters(
        &self,
        node_id: &str,
        public_keys: &[String],
        observed_at: i64,
    ) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM node_harvesters_current WHERE node_id = ?")
            .bind(node_id)
            .execute(&mut *tx)
            .await?;
        for public_key in public_keys {
            sqlx::query("INSERT OR IGNORE INTO node_harvesters_current (node_id, public_key, observed_at) VALUES (?, ?, ?)")
                .bind(node_id)
                .bind(public_key.to_ascii_uppercase())
                .bind(observed_at)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn harvest_nodes_for_public_key(&self, public_key: &str) -> AppResult<Vec<String>> {
        let rows = sqlx::query(
            "SELECT node_id FROM node_harvesters_current WHERE public_key = ? ORDER BY node_id",
        )
        .bind(public_key.to_ascii_uppercase())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("node_id").map_err(Into::into))
            .collect()
    }

    pub async fn record_harvest_event(&self, event: &HarvestEvent) -> AppResult<bool> {
        let event_key = event
            .hash
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                format!(
                    "{}:{}",
                    event.height.clone().unwrap_or_default(),
                    event.harvester_public_key
                )
            });
        let result = sqlx::query("INSERT OR IGNORE INTO harvest_events (id, event_key, node_id, observer_node_id, harvester_public_key, height, hash, observed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&event.id)
            .bind(event_key)
            .bind(&event.node_id)
            .bind(&event.observer_node_id)
            .bind(&event.harvester_public_key)
            .bind(&event.height)
            .bind(&event.hash)
            .bind(event.observed_at)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn recent_harvest_events(&self, limit: usize) -> AppResult<Vec<HarvestEvent>> {
        self.harvest_events_query(
            "SELECT * FROM harvest_events ORDER BY observed_at DESC LIMIT ?",
            None,
            limit,
        )
        .await
    }

    pub async fn recent_harvest_events_for_node(
        &self,
        node_id: &str,
        limit: usize,
    ) -> AppResult<Vec<HarvestEvent>> {
        self.harvest_events_query(
            "SELECT * FROM harvest_events WHERE node_id = ? ORDER BY observed_at DESC LIMIT ?",
            Some(node_id),
            limit,
        )
        .await
    }

    async fn harvest_events_query(
        &self,
        sql: &str,
        node_id: Option<&str>,
        limit: usize,
    ) -> AppResult<Vec<HarvestEvent>> {
        let mut query = sqlx::query(sql);
        if let Some(node_id) = node_id {
            query = query.bind(node_id);
        }
        let rows = query
            .bind(limit.min(200) as i64)
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(HarvestEvent {
                    id: row.try_get("id")?,
                    node_id: row.try_get("node_id")?,
                    observer_node_id: row.try_get("observer_node_id")?,
                    harvester_public_key: row.try_get("harvester_public_key")?,
                    height: row.try_get("height")?,
                    hash: row.try_get("hash")?,
                    observed_at: row.try_get("observed_at")?,
                })
            })
            .collect()
    }

    pub async fn latest_harvest_at(&self, node_id: &str) -> AppResult<Option<i64>> {
        let row = sqlx::query(
            "SELECT observed_at FROM harvest_events WHERE node_id = ? ORDER BY observed_at DESC LIMIT 1",
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| row.try_get("observed_at"))
            .transpose()
            .map_err(Into::into)
    }

    pub async fn replace_peers(
        &self,
        node_id: &str,
        peers: &[PeerInfo],
        observed_at: i64,
    ) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM peers_current WHERE node_id = ?")
            .bind(node_id)
            .execute(&mut *tx)
            .await?;
        for peer in peers {
            // Public node APIs can return the same peer more than once. Keep
            // the first occurrence (NIS1 orders active peers first) instead of
            // aborting the entire monitor cycle on the composite primary key.
            let roles = serde_json::to_string(&peer.roles)?;
            sqlx::query("INSERT OR IGNORE INTO peers_current (node_id, peer_key, name, host, port, status, version, roles_json, observed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(node_id).bind(&peer.key).bind(&peer.name).bind(&peer.host).bind(peer.port.map(i64::from))
                .bind(&peer.status).bind(&peer.version).bind(roles).bind(observed_at).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_peer_connections(&self) -> AppResult<Vec<(String, PeerInfo)>> {
        let rows = sqlx::query(
            "SELECT node_id, peer_key, name, host, port, status, version, roles_json FROM peers_current ORDER BY node_id, status, name",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    row.try_get("node_id")?,
                    PeerInfo {
                        key: row.try_get("peer_key")?,
                        name: row.try_get("name")?,
                        host: row.try_get("host")?,
                        port: row
                            .try_get::<Option<i64>, _>("port")?
                            .and_then(|value| u16::try_from(value).ok()),
                        status: row.try_get("status")?,
                        version: row.try_get("version")?,
                        roles: parse_string_list(row.try_get("roles_json")?),
                    },
                ))
            })
            .collect()
    }

    pub async fn latest_topology_health(&self) -> AppResult<HashMap<String, TopologyHealth>> {
        let rows = sqlx::query(
            "SELECT sample.node_id, sample.api_status, sample.db_status, sample.nis_status_code
             FROM node_samples sample
             JOIN (
               SELECT node_id, MAX(observed_at) AS observed_at
               FROM node_samples
               GROUP BY node_id
             ) latest ON latest.node_id = sample.node_id AND latest.observed_at = sample.observed_at",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    row.try_get("node_id")?,
                    TopologyHealth {
                        api_status: row.try_get("api_status")?,
                        db_status: row.try_get("db_status")?,
                        nis_status_code: row.try_get("nis_status_code")?,
                    },
                ))
            })
            .collect()
    }

    pub async fn node_detail(&self, id: &str, websocket_connected: bool) -> AppResult<NodeDetail> {
        let stored = self.get_stored_node(id).await?;
        let summary = self.node_summary(&stored).await?;
        let latest = sqlx::query(
            "SELECT * FROM node_samples WHERE node_id = ? ORDER BY observed_at DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let peer_rows =
            sqlx::query("SELECT * FROM peers_current WHERE node_id = ? ORDER BY status, name")
                .bind(id)
                .fetch_all(&self.pool)
                .await?;
        let peers = peer_rows
            .into_iter()
            .map(|row| {
                Ok(PeerInfo {
                    key: row.try_get("peer_key")?,
                    name: row.try_get("name")?,
                    host: row.try_get("host")?,
                    port: row
                        .try_get::<Option<i64>, _>("port")?
                        .and_then(|value| u16::try_from(value).ok()),
                    status: row.try_get("status")?,
                    version: row.try_get("version")?,
                    roles: parse_string_list(row.try_get("roles_json")?),
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        let recent_harvests = if summary.show_harvest_metrics {
            self.recent_harvest_events_for_node(id, 1).await?
        } else {
            Vec::new()
        };
        Ok(NodeDetail {
            node: summary,
            public_key: stored.public_key,
            roles: stored.roles,
            rest_version: stored.rest_version,
            chain_score: latest
                .as_ref()
                .and_then(|row| row.try_get("chain_score").ok())
                .flatten(),
            api_status: latest
                .as_ref()
                .and_then(|row| row.try_get("api_status").ok())
                .flatten(),
            db_status: latest
                .as_ref()
                .and_then(|row| row.try_get("db_status").ok())
                .flatten(),
            nis_status_code: latest
                .as_ref()
                .and_then(|row| row.try_get("nis_status_code").ok())
                .flatten(),
            peers,
            websocket_connected,
            recent_harvests,
        })
    }

    pub async fn query_metrics(
        &self,
        node_id: &str,
        from: i64,
        to: i64,
    ) -> AppResult<Vec<MetricPoint>> {
        let rows = sqlx::query("SELECT * FROM node_samples WHERE node_id = ? AND observed_at BETWEEN ? AND ? ORDER BY observed_at")
            .bind(node_id).bind(from).bind(to).fetch_all(&self.pool).await?;
        let mut harvest = self
            .latest_harvest_metrics_at_or_before(node_id, from)
            .await?;
        let mut certificate = self
            .latest_certificate_metrics_at_or_before(node_id, from)
            .await?;
        let mut supernode = self
            .latest_supernode_metrics_at_or_before(node_id, from)
            .await?;
        rows.into_iter()
            .map(|row| {
                harvest.delegated_harvester_count = row
                    .try_get::<Option<i64>, _>("delegated_harvester_count")?
                    .or(harvest.delegated_harvester_count);
                harvest.delegated_harvester_delta = row
                    .try_get::<Option<i64>, _>("delegated_harvester_delta")?
                    .or(harvest.delegated_harvester_delta);
                harvest.total_importance = row
                    .try_get::<Option<String>, _>("total_importance")?
                    .or_else(|| harvest.total_importance.clone());
                harvest.total_balance_micro_xym = row
                    .try_get::<Option<String>, _>("total_balance_micro_xym")?
                    .or_else(|| harvest.total_balance_micro_xym.clone());
                certificate.node_certificate_remaining_days = row
                    .try_get::<Option<i64>, _>("node_certificate_remaining_days")?
                    .or(certificate.node_certificate_remaining_days);
                certificate.voting_key_remaining_epochs = row
                    .try_get::<Option<i64>, _>("voting_key_remaining_epochs")?
                    .or(certificate.voting_key_remaining_epochs);
                supernode.supernode_checks_passed =
                    bool_from_i64(row.try_get::<Option<i64>, _>("supernode_checks_passed")?)
                        .or(supernode.supernode_checks_passed);
                supernode.supernode_last_round = row
                    .try_get::<Option<i64>, _>("supernode_last_round")?
                    .or(supernode.supernode_last_round);
                Ok(MetricPoint {
                    observed_at: row.try_get("observed_at")?,
                    height: row
                        .try_get::<Option<i64>, _>("height")?
                        .map(|value| value.to_string()),
                    finalized_height: row
                        .try_get::<Option<i64>, _>("finalized_height")?
                        .map(|value| value.to_string()),
                    lag: row
                        .try_get::<Option<i64>, _>("lag")?
                        .map(|value| value.to_string()),
                    peer_count: row.try_get("peer_count")?,
                    latency_ms: row.try_get("latency_ms")?,
                    reachable: row.try_get::<i64, _>("reachable")? != 0,
                    delegated_harvester_count: harvest.delegated_harvester_count,
                    delegated_harvester_delta: harvest.delegated_harvester_delta,
                    total_importance: harvest.total_importance.clone(),
                    total_balance_micro_xym: harvest.total_balance_micro_xym.clone(),
                    node_certificate_remaining_days: certificate.node_certificate_remaining_days,
                    voting_key_remaining_epochs: certificate.voting_key_remaining_epochs,
                    supernode_checks_passed: supernode.supernode_checks_passed,
                    supernode_last_round: supernode.supernode_last_round,
                })
            })
            .collect()
    }

    pub async fn load_settings(&self) -> AppResult<AppSettings> {
        let row = sqlx::query("SELECT value_json FROM settings WHERE key = 'app'")
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(row) => {
                let raw = row.try_get::<String, _>("value_json")?;
                let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
                // Existing installations predate onboarding. Mark only those
                // records complete while preserving every previously saved setting.
                if let Some(object) = value.as_object_mut() {
                    object
                        .entry("onboardingCompleted")
                        .or_insert(serde_json::Value::Bool(true));
                }
                Ok(serde_json::from_value(value).unwrap_or_default())
            }
            None => Ok(AppSettings::default()),
        }
    }

    pub async fn save_settings(&self, settings: &AppSettings) -> AppResult<()> {
        sqlx::query("INSERT INTO settings (key, value_json, updated_at) VALUES ('app', ?, ?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at")
            .bind(serde_json::to_string(settings)?).bind(chrono::Utc::now().timestamp_millis()).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn list_alerts(&self) -> AppResult<Vec<AlertRecord>> {
        let rows = sqlx::query("SELECT a.*, n.display_name FROM alerts a JOIN nodes n ON n.id = a.node_id ORDER BY CASE a.state WHEN 'open' THEN 0 WHEN 'acknowledged' THEN 1 ELSE 2 END, a.updated_at DESC LIMIT 1000")
            .fetch_all(&self.pool).await?;
        rows.iter().map(alert_from_row).collect()
    }

    pub async fn get_alert(&self, id: &str) -> AppResult<AlertRecord> {
        let row = sqlx::query("SELECT a.*, n.display_name FROM alerts a JOIN nodes n ON n.id = a.node_id WHERE a.id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| AppError::NotFound(id.into()))?;
        alert_from_row(&row)
    }

    pub async fn acknowledge_alert(&self, id: &str) -> AppResult<()> {
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query("UPDATE alerts SET state='acknowledged', acknowledged_at=?, updated_at=? WHERE id=? AND state='open'")
            .bind(now).bind(now).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn raise_alert(
        &self,
        node_id: &str,
        alert_type: &str,
        severity: &str,
        message: &str,
    ) -> AppResult<AlertMutation> {
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(row) = sqlx::query("SELECT id, severity FROM alerts WHERE node_id=? AND alert_type=? AND state!='resolved' ORDER BY raised_at DESC LIMIT 1")
            .bind(node_id).bind(alert_type).fetch_optional(&self.pool).await? {
            let id: String = row.try_get("id")?;
            let old_severity: String = row.try_get("severity")?;
            sqlx::query("UPDATE alerts SET severity=?, message=?, updated_at=?, occurrence_count=occurrence_count+1 WHERE id=?")
                .bind(severity).bind(message).bind(now).bind(&id).execute(&self.pool).await?;
            return Ok(AlertMutation {
                alert_id: id,
                changed: old_severity != severity,
            });
        }
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO alerts (id,node_id,alert_type,severity,state,message,raised_at,updated_at,occurrence_count) VALUES (?,?,?,?, 'open', ?,?,?,1)")
            .bind(&id).bind(node_id).bind(alert_type).bind(severity).bind(message).bind(now).bind(now)
            .execute(&self.pool).await?;
        Ok(AlertMutation {
            alert_id: id,
            changed: true,
        })
    }

    pub async fn resolve_alert(
        &self,
        node_id: &str,
        alert_type: &str,
    ) -> AppResult<Option<String>> {
        let now = chrono::Utc::now().timestamp_millis();
        let id = sqlx::query("SELECT id FROM alerts WHERE node_id=? AND alert_type=? AND state!='resolved' ORDER BY raised_at DESC LIMIT 1")
            .bind(node_id)
            .bind(alert_type)
            .fetch_optional(&self.pool)
            .await?
            .map(|row| row.try_get::<String, _>("id"))
            .transpose()?;
        let Some(id) = id else {
            return Ok(None);
        };
        let result = sqlx::query("UPDATE alerts SET state='resolved', resolved_at=?, updated_at=? WHERE node_id=? AND alert_type=? AND state!='resolved'")
            .bind(now).bind(now).bind(node_id).bind(alert_type).execute(&self.pool).await?;
        Ok((result.rows_affected() > 0).then_some(id))
    }

    pub async fn save_diagnostic_run(&self, run: &DiagnosticRun) -> AppResult<()> {
        sqlx::query("INSERT INTO diagnostic_runs (id,node_id,alert_id,started_at,completed_at,status,summary,checks_json) VALUES (?,?,?,?,?,?,?,?)")
            .bind(&run.id)
            .bind(&run.node_id)
            .bind(&run.alert_id)
            .bind(run.started_at)
            .bind(run.completed_at)
            .bind(run.status.as_str())
            .bind(&run.summary)
            .bind(serde_json::to_string(&run.checks)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_diagnostic_run(&self, id: &str) -> AppResult<DiagnosticRun> {
        let row = sqlx::query("SELECT * FROM diagnostic_runs WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| AppError::NotFound(id.into()))?;
        diagnostic_from_row(&row)
    }

    pub async fn list_node_diagnostics(
        &self,
        node_id: &str,
        limit: usize,
    ) -> AppResult<Vec<DiagnosticRun>> {
        let rows = sqlx::query(
            "SELECT * FROM diagnostic_runs WHERE node_id = ? ORDER BY completed_at DESC LIMIT ?",
        )
        .bind(node_id)
        .bind(limit.min(100) as i64)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(diagnostic_from_row).collect()
    }

    pub async fn compact(&self, settings: &AppSettings) -> AppResult<()> {
        let now = chrono::Utc::now().timestamp_millis();
        let raw_cutoff = now - settings.raw_retention_days as i64 * 86_400_000;
        let aggregate_cutoff = now - settings.aggregate_retention_days as i64 * 86_400_000;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT OR REPLACE INTO hourly_metrics
             SELECT node_id, (observed_at / 3600000) * 3600000, COUNT(*), SUM(reachable), AVG(latency_ms), MIN(latency_ms), MAX(latency_ms),
             MIN(height), MAX(height), MIN(finalized_height), MAX(finalized_height), AVG(peer_count), MIN(peer_count), MAX(peer_count), MAX(lag)
             FROM node_samples WHERE observed_at < ? GROUP BY node_id, (observed_at / 3600000)"
        ).bind(raw_cutoff).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM node_samples WHERE observed_at < ?")
            .bind(raw_cutoff)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM diagnostic_runs WHERE completed_at < ?")
            .bind(raw_cutoff)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM hourly_metrics WHERE bucket_at < ?")
            .bind(aggregate_cutoff)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol_identity() -> NodeIdentity {
        NodeIdentity {
            kind: NodeKind::Symbol,
            name: None,
            network: Some("Symbol Mainnet".into()),
            version: Some("1.0.0".into()),
            public_key: None,
            roles: Vec::new(),
            rest_version: Some("2.4.0".into()),
        }
    }

    async fn node_ids(repository: &Repository) -> Vec<String> {
        repository
            .list_stored_nodes(false)
            .await
            .expect("nodes should be listed")
            .into_iter()
            .map(|node| node.id)
            .collect()
    }

    fn sample_at(observed_at: i64) -> NodeSample {
        NodeSample {
            observed_at,
            reachable: true,
            status: NodeStatus::Healthy,
            latency_ms: Some(100),
            height: Some(100),
            finalized_height: Some(90),
            network_height: Some(100),
            lag: Some(0),
            peer_count: Some(5),
            chain_score: None,
            api_status: None,
            db_status: None,
            nis_status_code: None,
            last_error: None,
            delegated_harvester_count: None,
            delegated_harvester_delta: None,
            total_importance: None,
            total_balance_micro_xym: None,
            harvest_metric_error: None,
            node_certificate_not_after: None,
            node_certificate_remaining_days: None,
            voting_key_start_epoch: None,
            voting_key_end_epoch: None,
            finalization_epoch: None,
            voting_key_remaining_epochs: None,
            certificate_metric_error: None,
            supernode_id: None,
            supernode_status: None,
            supernode_last_round: None,
            supernode_last_round_at: None,
            supernode_checks_passed: None,
            supernode_failed_checks: Vec::new(),
            supernode_metric_error: None,
        }
    }

    #[tokio::test]
    async fn persists_manual_node_order() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let database = directory.path().join("node-watcher.db");
        let repository = Repository::open(&database)
            .await
            .expect("repository should open");
        let identity = symbol_identity();
        for (id, name) in [
            ("node-b", "Node B"),
            ("node-a", "Node A"),
            ("node-c", "Node C"),
        ] {
            repository
                .upsert_node(
                    id,
                    &format!("http://{id}:3000"),
                    name,
                    &identity,
                    true,
                    false,
                )
                .await
                .expect("node should be inserted");
        }

        assert_eq!(node_ids(&repository).await, ["node-b", "node-a", "node-c"]);
        repository
            .move_node("node-c", -1)
            .await
            .expect("node should move up");
        assert_eq!(node_ids(&repository).await, ["node-b", "node-c", "node-a"]);

        drop(repository);
        let reopened = Repository::open(&database)
            .await
            .expect("repository should reopen");
        assert_eq!(node_ids(&reopened).await, ["node-b", "node-c", "node-a"]);
    }

    #[tokio::test]
    async fn move_node_handles_boundaries_and_unknown_nodes() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let repository = Repository::open(&directory.path().join("node-watcher.db"))
            .await
            .expect("repository should open");
        repository
            .upsert_node(
                "node-a",
                "http://node-a:3000",
                "Node A",
                &symbol_identity(),
                true,
                false,
            )
            .await
            .expect("node should be inserted");

        repository
            .move_node("node-a", -1)
            .await
            .expect("moving the first node up should be a no-op");
        assert_eq!(node_ids(&repository).await, ["node-a"]);
        assert!(matches!(
            repository.move_node("missing", 1).await,
            Err(AppError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn duplicate_peer_keys_do_not_abort_snapshot_replacement() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let repository = Repository::open(&directory.path().join("node-watcher.db"))
            .await
            .expect("repository should open");
        repository
            .upsert_node(
                "node-a",
                "http://node-a:3000",
                "Node A",
                &symbol_identity(),
                true,
                false,
            )
            .await
            .expect("node should be inserted");
        let peers = vec![
            PeerInfo {
                key: "duplicate-key".into(),
                name: Some("Active peer".into()),
                host: Some("192.0.2.1".into()),
                port: Some(7900),
                status: "active".into(),
                version: None,
                roles: Vec::new(),
            },
            PeerInfo {
                key: "duplicate-key".into(),
                name: Some("Duplicate peer".into()),
                host: Some("192.0.2.2".into()),
                port: Some(7900),
                status: "inactive".into(),
                version: None,
                roles: Vec::new(),
            },
        ];

        repository
            .replace_peers("node-a", &peers, 1)
            .await
            .expect("duplicate peer keys should be accepted");
        let detail = repository
            .node_detail("node-a", false)
            .await
            .expect("node detail should load");

        assert_eq!(detail.peers.len(), 1);
        assert_eq!(detail.peers[0].name.as_deref(), Some("Active peer"));
        assert_eq!(detail.peers[0].status, "active");
    }

    #[tokio::test]
    async fn latest_node_summary_keeps_previous_harvest_metrics_when_new_sample_has_none() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let repository = Repository::open(&directory.path().join("node-watcher.db"))
            .await
            .expect("repository should open");
        repository
            .upsert_node(
                "node-a",
                "http://node-a:3000",
                "Node A",
                &symbol_identity(),
                true,
                false,
            )
            .await
            .expect("node should be inserted");
        repository
            .set_harvest_metrics_visible("node-a", true)
            .await
            .expect("harvest metrics should be enabled");

        let mut harvest_sample = sample_at(1);
        harvest_sample.delegated_harvester_count = Some(3);
        harvest_sample.delegated_harvester_delta = Some(1);
        harvest_sample.total_importance = Some("123".into());
        harvest_sample.total_balance_micro_xym = Some("9876543210".into());
        harvest_sample.node_certificate_not_after = Some(1_800_000_000_000);
        harvest_sample.node_certificate_remaining_days = Some(12);
        harvest_sample.voting_key_start_epoch = Some(100);
        harvest_sample.voting_key_end_epoch = Some(150);
        harvest_sample.finalization_epoch = Some(142);
        harvest_sample.voting_key_remaining_epochs = Some(8);
        harvest_sample.supernode_id = Some(72052155);
        harvest_sample.supernode_status = Some("active".into());
        harvest_sample.supernode_last_round = Some(4956);
        harvest_sample.supernode_last_round_at = Some(1_783_613_676_463);
        harvest_sample.supernode_checks_passed = Some(false);
        harvest_sample.supernode_failed_checks = vec!["bandwidth".into()];
        repository
            .save_sample("node-a", &harvest_sample)
            .await
            .expect("harvest sample should be saved");

        let regular_sample = sample_at(2);
        repository
            .save_sample("node-a", &regular_sample)
            .await
            .expect("regular sample should be saved");

        let summary = repository
            .list_nodes()
            .await
            .expect("nodes should be listed")
            .pop()
            .expect("node should exist");
        assert_eq!(summary.delegated_harvester_count, Some(3));
        assert_eq!(summary.delegated_harvester_delta, Some(1));
        assert_eq!(summary.total_importance.as_deref(), Some("123"));
        assert_eq!(
            summary.total_balance_micro_xym.as_deref(),
            Some("9876543210")
        );
        assert_eq!(summary.node_certificate_not_after, Some(1_800_000_000_000));
        assert_eq!(summary.node_certificate_remaining_days, Some(12));
        assert_eq!(summary.voting_key_end_epoch, Some(150));
        assert_eq!(summary.voting_key_remaining_epochs, Some(8));
        assert_eq!(summary.supernode_id, Some(72052155));
        assert_eq!(summary.supernode_status.as_deref(), Some("active"));
        assert_eq!(summary.supernode_last_round, Some(4956));
        assert_eq!(summary.supernode_checks_passed, Some(false));
        assert_eq!(summary.supernode_failed_checks, ["bandwidth"]);
        let metrics = repository
            .query_metrics("node-a", 1, 2)
            .await
            .expect("metrics should be listed");
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].delegated_harvester_count, Some(3));
        assert_eq!(metrics[1].delegated_harvester_count, Some(3));
        assert_eq!(
            metrics[1].total_balance_micro_xym.as_deref(),
            Some("9876543210")
        );
        assert_eq!(metrics[1].node_certificate_remaining_days, Some(12));
        assert_eq!(metrics[1].voting_key_remaining_epochs, Some(8));
        assert_eq!(metrics[0].supernode_checks_passed, Some(false));
        assert_eq!(metrics[1].supernode_last_round, Some(4956));
    }

    #[tokio::test]
    async fn alert_mutations_return_a_stable_alert_id() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let repository = Repository::open(&directory.path().join("node-watcher.db"))
            .await
            .expect("repository should open");
        repository
            .upsert_node(
                "node-a",
                "http://node-a:3000",
                "Node A",
                &symbol_identity(),
                true,
                false,
            )
            .await
            .expect("node should be inserted");

        let raised = repository
            .raise_alert("node-a", "offline", "critical", "offline")
            .await
            .expect("alert should be raised");
        assert!(raised.changed);
        let repeated = repository
            .raise_alert("node-a", "offline", "critical", "offline")
            .await
            .expect("alert should be updated");
        assert!(!repeated.changed);
        assert_eq!(repeated.alert_id, raised.alert_id);
        assert_eq!(
            repository.resolve_alert("node-a", "offline").await.unwrap(),
            Some(raised.alert_id)
        );
    }

    #[tokio::test]
    async fn preserves_legacy_settings_and_skips_onboarding() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let repository = Repository::open(&directory.path().join("node-watcher.db"))
            .await
            .expect("repository should open");
        sqlx::query("INSERT INTO settings (key,value_json,updated_at) VALUES ('app', ?, 1)")
            .bind(r#"{"healthIntervalSeconds":25,"backgroundMonitoring":false,"autostartEnabled":true}"#)
            .execute(&repository.pool)
            .await
            .unwrap();

        let settings = repository.load_settings().await.unwrap();
        assert_eq!(settings.health_interval_seconds, 25);
        assert!(!settings.background_monitoring);
        assert!(settings.autostart_enabled);
        assert!(settings.onboarding_completed);
    }

    #[tokio::test]
    async fn persists_and_expires_diagnostic_runs() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let repository = Repository::open(&directory.path().join("node-watcher.db"))
            .await
            .expect("repository should open");
        repository
            .upsert_node(
                "node-a",
                "http://node-a:3000",
                "Node A",
                &symbol_identity(),
                true,
                false,
            )
            .await
            .expect("node should be inserted");
        let run = DiagnosticRun {
            id: "run-a".into(),
            node_id: "node-a".into(),
            alert_id: None,
            started_at: 0,
            completed_at: 0,
            status: DiagnosticStatus::Failed,
            summary: "failed".into(),
            checks: vec![],
        };
        repository.save_diagnostic_run(&run).await.unwrap();
        assert_eq!(
            repository
                .list_node_diagnostics("node-a", 20)
                .await
                .unwrap()
                .len(),
            1
        );

        repository
            .compact(&AppSettings {
                raw_retention_days: 1,
                ..AppSettings::default()
            })
            .await
            .unwrap();
        assert!(repository
            .list_node_diagnostics("node-a", 20)
            .await
            .unwrap()
            .is_empty());
    }
}

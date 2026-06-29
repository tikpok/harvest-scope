use std::{path::Path, str::FromStr, time::Duration};

use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, SqlitePool,
};

use crate::{
    domain::{
        AlertRecord, AppSettings, MetricPoint, NodeDetail, NodeIdentity, NodeKind, NodeSample,
        NodeStatus, NodeSummary, PeerInfo, StoredNode,
    },
    error::{AppError, AppResult},
};

#[derive(Clone)]
pub struct Repository {
    pool: SqlitePool,
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

fn to_i64(value: Option<u64>) -> Option<i64> {
    value.and_then(|value| i64::try_from(value).ok())
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
            "SELECT * FROM nodes WHERE enabled = 1 ORDER BY favorite DESC, display_name"
        } else {
            "SELECT * FROM nodes ORDER BY favorite DESC, display_name"
        };
        let rows = sqlx::query(sql).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(StoredNode {
                    id: row.try_get("id")?,
                    display_name: row.try_get("display_name")?,
                    kind: parse_kind(row.try_get::<String, _>("kind")?.as_str())?,
                    base_url: row.try_get("base_url")?,
                    network: row.try_get("network")?,
                    version: row.try_get("version")?,
                    public_key: row.try_get("public_key")?,
                    roles: serde_json::from_str(&row.try_get::<String, _>("roles_json")?)
                        .unwrap_or_default(),
                    rest_version: row.try_get("rest_version")?,
                    enabled: row.try_get::<i64, _>("enabled")? != 0,
                    favorite: row.try_get::<i64, _>("favorite")? != 0,
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
        for node in nodes {
            let sample = sqlx::query(
                "SELECT * FROM node_samples WHERE node_id = ? ORDER BY observed_at DESC LIMIT 1",
            )
            .bind(&node.id)
            .fetch_optional(&self.pool)
            .await?;
            let summary = if let Some(row) = sample {
                NodeSummary {
                    id: node.id,
                    display_name: node.display_name,
                    kind: node.kind,
                    base_url: node.base_url,
                    network: node.network,
                    version: node.version,
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
                }
            } else {
                NodeSummary {
                    id: node.id,
                    display_name: node.display_name,
                    kind: node.kind,
                    base_url: node.base_url,
                    network: node.network,
                    version: node.version,
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
                }
            };
            summaries.push(summary);
        }
        Ok(summaries)
    }

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
            "INSERT INTO nodes (id, display_name, kind, base_url, network, version, public_key, roles_json, rest_version, enabled, favorite, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET display_name=excluded.display_name, kind=excluded.kind, base_url=excluded.base_url,
             network=excluded.network, version=excluded.version, public_key=excluded.public_key, roles_json=excluded.roles_json,
             rest_version=excluded.rest_version, enabled=excluded.enabled, favorite=excluded.favorite, updated_at=excluded.updated_at"
        ).bind(id).bind(display_name).bind(identity.kind.as_str()).bind(base_url).bind(&identity.network).bind(&identity.version)
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

    pub async fn delete_node(&self, id: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM nodes WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn save_sample(&self, node_id: &str, sample: &NodeSample) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO node_samples (node_id, observed_at, reachable, status, latency_ms, height, finalized_height, network_height, lag, peer_count, chain_score, api_status, db_status, nis_status_code, last_error)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        ).bind(node_id).bind(sample.observed_at).bind(sample.reachable as i64).bind(sample.status.as_str())
            .bind(sample.latency_ms).bind(to_i64(sample.height)).bind(to_i64(sample.finalized_height))
            .bind(to_i64(sample.network_height)).bind(to_i64(sample.lag)).bind(sample.peer_count)
            .bind(&sample.chain_score).bind(&sample.api_status).bind(&sample.db_status).bind(sample.nis_status_code)
            .bind(&sample.last_error).execute(&self.pool).await?;
        Ok(())
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
            sqlx::query("INSERT INTO peers_current (node_id, peer_key, name, host, port, status, version, observed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(node_id).bind(&peer.key).bind(&peer.name).bind(&peer.host).bind(peer.port.map(i64::from))
                .bind(&peer.status).bind(&peer.version).bind(observed_at).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn node_detail(&self, id: &str, websocket_connected: bool) -> AppResult<NodeDetail> {
        let stored = self.get_stored_node(id).await?;
        let summary = self
            .list_nodes()
            .await?
            .into_iter()
            .find(|node| node.id == id)
            .ok_or_else(|| AppError::NotFound(id.into()))?;
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
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
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
        rows.into_iter()
            .map(|row| {
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
                })
            })
            .collect()
    }

    pub async fn load_settings(&self) -> AppResult<AppSettings> {
        let row = sqlx::query("SELECT value_json FROM settings WHERE key = 'app'")
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(row) => Ok(
                serde_json::from_str(&row.try_get::<String, _>("value_json")?).unwrap_or_default(),
            ),
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
        rows.into_iter()
            .map(|row| {
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
            })
            .collect()
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
    ) -> AppResult<bool> {
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(row) = sqlx::query("SELECT id, severity FROM alerts WHERE node_id=? AND alert_type=? AND state!='resolved' ORDER BY raised_at DESC LIMIT 1")
            .bind(node_id).bind(alert_type).fetch_optional(&self.pool).await? {
            let id: String = row.try_get("id")?;
            let old_severity: String = row.try_get("severity")?;
            sqlx::query("UPDATE alerts SET severity=?, message=?, updated_at=?, occurrence_count=occurrence_count+1 WHERE id=?")
                .bind(severity).bind(message).bind(now).bind(id).execute(&self.pool).await?;
            return Ok(old_severity != severity);
        }
        sqlx::query("INSERT INTO alerts (id,node_id,alert_type,severity,state,message,raised_at,updated_at,occurrence_count) VALUES (?,?,?,?, 'open', ?,?,?,1)")
            .bind(uuid::Uuid::new_v4().to_string()).bind(node_id).bind(alert_type).bind(severity).bind(message).bind(now).bind(now)
            .execute(&self.pool).await?;
        Ok(true)
    }

    pub async fn resolve_alert(&self, node_id: &str, alert_type: &str) -> AppResult<bool> {
        let now = chrono::Utc::now().timestamp_millis();
        let result = sqlx::query("UPDATE alerts SET state='resolved', resolved_at=?, updated_at=? WHERE node_id=? AND alert_type=? AND state!='resolved'")
            .bind(now).bind(now).bind(node_id).bind(alert_type).execute(&self.pool).await?;
        Ok(result.rows_affected() > 0)
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
        sqlx::query("DELETE FROM hourly_metrics WHERE bucket_at < ?")
            .bind(aggregate_cutoff)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

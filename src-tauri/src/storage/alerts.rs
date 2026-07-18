use sqlx::{sqlite::SqliteRow, Row};

use crate::{
    domain::{AlertMutation, AlertRecord},
    error::{AppError, AppResult},
};

use super::Repository;

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

impl Repository {
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
}

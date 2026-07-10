use sqlx::{sqlite::SqliteRow, Row};

use crate::{
    domain::{DiagnosticRun, DiagnosticStatus},
    error::{AppError, AppResult},
};

use super::Repository;

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


}

use tauri::{AppHandle, Emitter};

use crate::{domain::StoredNode, error::AppResult, state::AppState};

use super::desktop;

pub(super) fn certificate_expiry_severity(remaining: Option<i64>) -> Option<&'static str> {
    remaining.and_then(|value| {
        if value <= 7 {
            Some("critical")
        } else if value <= 30 {
            Some("warning")
        } else {
            None
        }
    })
}

pub(super) fn node_certificate_message(days: i64) -> String {
    if days < 0 {
        "ノード証明書の期限が切れています。".into()
    } else {
        format!("ノード証明書の期限まで残り{days}日です。")
    }
}

pub(super) fn voting_key_message(epochs: i64) -> String {
    if epochs == 0 {
        "投票キーの有効epochが終了しています。".into()
    } else {
        format!("投票キーの有効期限まで残り{epochs} epochです。")
    }
}

pub(super) fn supernode_failure_message(round: Option<i64>, failed_checks: &[String]) -> String {
    let round = round
        .map(|value| format!("ラウンド{value}"))
        .unwrap_or_else(|| "最新ラウンド".into());
    if failed_checks.is_empty() {
        format!("NEM Supernodeの{round}でチェックに失敗しました。")
    } else {
        format!(
            "NEM Supernodeの{round}で{}に失敗しました。",
            failed_checks.join(", ")
        )
    }
}

pub(super) async fn update_alert(
    app: &AppHandle,
    state: &AppState,
    node: &StoredNode,
    active: bool,
    alert_type: &str,
    severity: &str,
    message: &str,
) -> AppResult<()> {
    let mutation = if active {
        let mutation = state
            .repository
            .raise_alert(&node.id, alert_type, severity, message)
            .await?;
        mutation.changed.then_some(mutation.alert_id)
    } else {
        state.repository.resolve_alert(&node.id, alert_type).await?
    };
    if let Some(alert_id) = mutation {
        let title = if active {
            format!(
                "{}: {}",
                if severity == "critical" {
                    "重大"
                } else {
                    "警告"
                },
                node.display_name
            )
        } else {
            format!("復旧: {}", node.display_name)
        };
        desktop::notify_alert(app.clone(), state.clone(), &title, message, alert_id).await;
        let _ = app.emit("alert-changed", &node.id);
        desktop::refresh_tray(app, state).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificate_expiry_thresholds_are_warning_then_critical() {
        assert_eq!(certificate_expiry_severity(Some(31)), None);
        assert_eq!(certificate_expiry_severity(Some(30)), Some("warning"));
        assert_eq!(certificate_expiry_severity(Some(7)), Some("critical"));
        assert_eq!(certificate_expiry_severity(Some(-1)), Some("critical"));
        assert_eq!(certificate_expiry_severity(None), None);
    }
}

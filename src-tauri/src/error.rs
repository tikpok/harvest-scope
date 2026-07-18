use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("URLが正しくありません: {0}")]
    InvalidUrl(String),
    #[error("ネットワークエラー: {0}")]
    Network(String),
    #[error("安全でないリダイレクトを拒否しました: {0}")]
    RedirectRejected(String),
    #[error("APIレスポンスが上限を超えました（上限: {limit} bytes）")]
    ResponseTooLarge { limit: usize },
    #[error("HTTP接続には明示的な確認が必要です: {0}")]
    InsecureHttpConfirmationRequired(String),
    #[error("Supernode IDが競合しています（URL: {endpoint_id} / 手動指定: {configured_id}）")]
    SupernodeIdentityConflict {
        endpoint_id: i64,
        configured_id: i64,
    },
    #[error("ノードの種類を判定できませんでした（Symbol: {symbol} / NIS1: {nis1}）")]
    UnknownNode { symbol: String, nis1: String },
    #[error("APIレスポンスを解釈できません: {0}")]
    InvalidPayload(String),
    #[error("データベースエラー: {0}")]
    Database(#[from] sqlx::Error),
    #[error("DBマイグレーションエラー: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("データ変換エラー: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("CSVエラー: {0}")]
    Csv(#[from] csv::Error),
    #[error("設定エラー: {0}")]
    Configuration(String),
    #[error("操作をキャンセルしました")]
    Cancelled,
    #[error("対象が見つかりません: {0}")]
    NotFound(String),
    #[error("入出力エラー: {0}")]
    Io(#[from] std::io::Error),
}

impl From<reqwest::Error> for AppError {
    fn from(value: reqwest::Error) -> Self {
        Self::Network(if value.is_timeout() {
            "タイムアウトしました".into()
        } else if value.is_connect() {
            format!("接続できません: {value}")
        } else {
            value.to_string()
        })
    }
}

impl From<url::ParseError> for AppError {
    fn from(value: url::ParseError) -> Self {
        Self::InvalidUrl(value.to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

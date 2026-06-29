use async_trait::async_trait;

use crate::error::AppResult;

use super::{ChainSnapshot, HealthSnapshot, NodeIdentity, PeerInfo, WebSocketSpec};

#[async_trait]
pub trait NodeAdapter: Send + Sync {
    async fn probe(&self) -> AppResult<NodeIdentity>;
    async fn fetch_health(&self) -> AppResult<HealthSnapshot>;
    async fn fetch_chain(&self) -> AppResult<ChainSnapshot>;
    async fn fetch_peers(&self) -> AppResult<Vec<PeerInfo>>;
    async fn fetch_static_info(&self) -> AppResult<NodeIdentity> {
        self.probe().await
    }
    async fn fetch_state_hash(&self, _height: u64) -> AppResult<Option<String>> {
        Ok(None)
    }
    fn websocket_capability(&self) -> Option<WebSocketSpec> {
        None
    }
}

use async_trait::async_trait;

use crate::error::AppResult;

use super::{
    CertificateSnapshot, ChainSnapshot, HarvestSnapshot, HealthSnapshot, NodeIdentity, PeerInfo,
    SupernodeProgramNode, SupernodeSnapshot, WebSocketSpec,
};

// async-trait adds #[must_use] to futures that Rust 1.99 already treats as must-use.
#[async_trait]
#[allow(clippy::double_must_use)]
pub trait NodeAdapter: Send + Sync {
    async fn probe(&self) -> AppResult<NodeIdentity>;
    async fn fetch_health(&self) -> AppResult<HealthSnapshot>;
    async fn fetch_chain(&self) -> AppResult<ChainSnapshot>;
    async fn fetch_peers(&self) -> AppResult<Vec<PeerInfo>>;
    async fn fetch_harvest_metrics(&self) -> AppResult<HarvestSnapshot> {
        Ok(HarvestSnapshot::default())
    }
    async fn fetch_certificate_metrics(&self) -> AppResult<CertificateSnapshot> {
        Ok(CertificateSnapshot::default())
    }
    async fn fetch_supernode_metrics(
        &self,
        _supernodes: &[SupernodeProgramNode],
        _configured_supernode_id: Option<i64>,
    ) -> AppResult<SupernodeSnapshot> {
        Ok(SupernodeSnapshot::default())
    }
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

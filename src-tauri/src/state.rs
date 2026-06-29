use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use reqwest::Client;
use tokio::sync::{Mutex, RwLock, Semaphore};

use crate::{
    domain::{AppSettings, ChainSnapshot, PeerInfo},
    storage::Repository,
};

#[derive(Debug, Clone, Default)]
pub struct NodeRuntime {
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    pub latency_failures: u32,
    pub latency_successes: u32,
    pub zero_peers_since: Option<i64>,
    pub last_chain_poll: i64,
    pub last_peer_poll: i64,
    pub last_static_poll: i64,
    pub chain: ChainSnapshot,
    pub peers: Vec<PeerInfo>,
    pub websocket_connected: bool,
    pub last_finalized_height: Option<u64>,
    pub finalized_changed_at: Option<i64>,
    pub height_at_finalization_change: Option<u64>,
    pub lag_warning_since: Option<i64>,
    pub lag_critical_since: Option<i64>,
}

#[derive(Clone)]
pub struct AppState {
    pub repository: Repository,
    pub client: Client,
    pub settings: Arc<RwLock<AppSettings>>,
    pub runtime: Arc<Mutex<HashMap<String, NodeRuntime>>>,
    pub websocket_started: Arc<Mutex<HashSet<String>>>,
    pub scan_cancelled: Arc<AtomicBool>,
    pub background_monitoring: Arc<AtomicBool>,
    pub state_hash_mismatches: Arc<Mutex<HashMap<String, u32>>>,
    pub monitoring_nodes: Arc<Mutex<HashSet<String>>>,
    pub request_limit: Arc<Semaphore>,
}

impl AppState {
    pub async fn new(repository: Repository, client: Client) -> crate::error::AppResult<Self> {
        let settings = repository.load_settings().await?;
        Ok(Self {
            repository,
            client,
            background_monitoring: Arc::new(AtomicBool::new(settings.background_monitoring)),
            settings: Arc::new(RwLock::new(settings)),
            runtime: Arc::new(Mutex::new(HashMap::new())),
            websocket_started: Arc::new(Mutex::new(HashSet::new())),
            scan_cancelled: Arc::new(AtomicBool::new(false)),
            state_hash_mismatches: Arc::new(Mutex::new(HashMap::new())),
            monitoring_nodes: Arc::new(Mutex::new(HashSet::new())),
            request_limit: Arc::new(Semaphore::new(16)),
        })
    }

    pub fn set_background_monitoring(&self, enabled: bool) {
        self.background_monitoring.store(enabled, Ordering::Relaxed);
    }
}

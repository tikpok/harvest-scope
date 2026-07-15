use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
        Arc,
    },
};

use reqwest::Client;
use tokio::sync::{Mutex, RwLock, Semaphore};

use crate::{
    domain::{AppSettings, ChainSnapshot, NetworkEvent, PeerInfo, SupernodeListCache},
    storage::Repository,
};

#[derive(Debug, Clone, Default)]
pub struct NodeRuntime {
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    pub offline_candidate_since: Option<i64>,
    pub offline_confirmation_failures: u32,
    pub latency_failures: u32,
    pub latency_successes: u32,
    pub zero_peers_since: Option<i64>,
    pub last_chain_poll: i64,
    pub last_peer_poll: i64,
    pub last_static_poll: i64,
    pub last_supernode_poll: i64,
    pub chain: ChainSnapshot,
    pub peers: Vec<PeerInfo>,
    pub websocket_connected: bool,
    pub last_finalized_height: Option<u64>,
    pub finalized_changed_at: Option<i64>,
    pub height_at_finalization_change: Option<u64>,
    pub lag_warning_since: Option<i64>,
    pub lag_critical_since: Option<i64>,
}

impl NodeRuntime {
    pub fn record_connectivity_success(&mut self, paused: bool) {
        if paused {
            self.reset_connectivity_counters();
            return;
        }

        self.consecutive_successes += 1;
        self.consecutive_failures = 0;
        self.offline_candidate_since = None;
        self.offline_confirmation_failures = 0;
    }

    pub fn record_connectivity_failure(
        &mut self,
        paused: bool,
        now: i64,
        confirmation_delay_ms: i64,
    ) {
        if paused {
            self.reset_connectivity_counters();
            return;
        }

        self.consecutive_failures += 1;
        self.consecutive_successes = 0;
        match self.offline_candidate_since {
            Some(since) if now.saturating_sub(since) >= confirmation_delay_ms => {
                self.offline_confirmation_failures += 1;
            }
            Some(_) => {
                self.offline_confirmation_failures = 0;
            }
            None => {
                self.offline_candidate_since = Some(now);
                self.offline_confirmation_failures = 0;
            }
        }
    }

    pub fn offline_confirmed(&self, required_failures: u32) -> bool {
        self.offline_confirmation_failures >= required_failures
    }

    fn reset_connectivity_counters(&mut self) {
        self.consecutive_failures = 0;
        self.consecutive_successes = 0;
        self.offline_candidate_since = None;
        self.offline_confirmation_failures = 0;
    }

    pub fn reset_after_resume(&mut self, now: i64) {
        self.reset_connectivity_counters();
        self.latency_failures = 0;
        self.latency_successes = 0;
        self.zero_peers_since = None;
        self.lag_warning_since = None;
        self.lag_critical_since = None;
        self.finalized_changed_at = Some(now);
        self.height_at_finalization_change = self.chain.height;
    }
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
    pub diagnostic_nodes: Arc<Mutex<HashSet<String>>>,
    pub request_limit: Arc<Semaphore>,
    pub network_events: Arc<Mutex<VecDeque<NetworkEvent>>>,
    pub pending_navigation: Arc<Mutex<Option<String>>>,
    pub resume_grace_until: Arc<AtomicI64>,
    pub system_suspended: Arc<AtomicBool>,
    pub all_nodes_rechecking: Arc<AtomicBool>,
    pub all_nodes_refresh_lock: Arc<Mutex<()>>,
    pub node_registration_lock: Arc<Mutex<()>>,
    pub supernode_nodes_cache: Arc<Mutex<Option<SupernodeListCache>>>,
    node_deletion_generation: Arc<AtomicU64>,
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
            diagnostic_nodes: Arc::new(Mutex::new(HashSet::new())),
            request_limit: Arc::new(Semaphore::new(16)),
            network_events: Arc::new(Mutex::new(VecDeque::with_capacity(500))),
            pending_navigation: Arc::new(Mutex::new(None)),
            resume_grace_until: Arc::new(AtomicI64::new(0)),
            system_suspended: Arc::new(AtomicBool::new(false)),
            all_nodes_rechecking: Arc::new(AtomicBool::new(false)),
            all_nodes_refresh_lock: Arc::new(Mutex::new(())),
            node_registration_lock: Arc::new(Mutex::new(())),
            supernode_nodes_cache: Arc::new(Mutex::new(None)),
            node_deletion_generation: Arc::new(AtomicU64::new(0)),
        })
    }

    pub fn note_node_deletion(&self) -> u64 {
        self.node_deletion_generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
    }

    pub fn current_node_deletion_generation(&self) -> u64 {
        self.node_deletion_generation.load(Ordering::Acquire)
    }

    pub fn set_background_monitoring(&self, enabled: bool) {
        self.background_monitoring.store(enabled, Ordering::Relaxed);
    }

    pub fn begin_resume_grace(&self, now: i64, duration_ms: i64) -> bool {
        let until = now.saturating_add(duration_ms);
        self.resume_grace_until
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                (current <= now).then_some(until)
            })
            .is_ok()
    }

    pub fn in_resume_grace(&self, now: i64) -> bool {
        now < self.resume_grace_until.load(Ordering::Relaxed)
    }

    pub fn set_system_suspended(&self, suspended: bool) {
        self.system_suspended.store(suspended, Ordering::Release);
    }

    pub fn is_system_suspended(&self) -> bool {
        self.system_suspended.load(Ordering::Acquire)
    }

    pub fn set_all_nodes_rechecking(&self, rechecking: bool) {
        self.all_nodes_rechecking
            .store(rechecking, Ordering::Release);
    }

    pub fn is_all_nodes_rechecking(&self) -> bool {
        self.all_nodes_rechecking.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::NodeRuntime;

    #[test]
    fn resume_reset_clears_transient_failure_state() {
        let mut runtime = NodeRuntime {
            consecutive_failures: 3,
            consecutive_successes: 2,
            offline_candidate_since: Some(1),
            offline_confirmation_failures: 3,
            latency_failures: 3,
            latency_successes: 3,
            zero_peers_since: Some(1),
            lag_warning_since: Some(1),
            lag_critical_since: Some(1),
            finalized_changed_at: Some(1),
            ..Default::default()
        };

        runtime.reset_after_resume(10_000);

        assert_eq!(runtime.consecutive_failures, 0);
        assert_eq!(runtime.consecutive_successes, 0);
        assert_eq!(runtime.offline_candidate_since, None);
        assert_eq!(runtime.offline_confirmation_failures, 0);
        assert_eq!(runtime.latency_failures, 0);
        assert_eq!(runtime.latency_successes, 0);
        assert_eq!(runtime.zero_peers_since, None);
        assert_eq!(runtime.lag_warning_since, None);
        assert_eq!(runtime.lag_critical_since, None);
        assert_eq!(runtime.finalized_changed_at, Some(10_000));
    }

    #[test]
    fn offline_confirmation_requires_delay_then_more_failures() {
        let mut runtime = NodeRuntime::default();
        for now in [0, 10_000, 20_000, 80_000] {
            runtime.record_connectivity_failure(false, now, 90_000);
            assert!(!runtime.offline_confirmed(3));
        }

        runtime.record_connectivity_failure(false, 90_000, 90_000);
        assert_eq!(runtime.offline_confirmation_failures, 1);
        assert!(!runtime.offline_confirmed(3));

        runtime.record_connectivity_failure(false, 100_000, 90_000);
        assert_eq!(runtime.offline_confirmation_failures, 2);
        assert!(!runtime.offline_confirmed(3));

        runtime.record_connectivity_failure(false, 110_000, 90_000);
        assert!(runtime.offline_confirmed(3));
    }

    #[test]
    fn success_clears_offline_candidate_state() {
        let mut runtime = NodeRuntime::default();
        runtime.record_connectivity_failure(false, 0, 90_000);
        runtime.record_connectivity_failure(false, 90_000, 90_000);
        runtime.record_connectivity_success(false);

        assert_eq!(runtime.consecutive_failures, 0);
        assert_eq!(runtime.consecutive_successes, 1);
        assert_eq!(runtime.offline_candidate_since, None);
        assert_eq!(runtime.offline_confirmation_failures, 0);
    }
}

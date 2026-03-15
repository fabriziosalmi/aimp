use serde::Deserialize;
use config::{Config, ConfigError, File, Environment};

#[derive(Debug, Deserialize, Clone)]
pub struct AimpConfig {
    pub port: u16,
    pub network_log_capacity: usize,
    pub max_visible_logs: usize,
    pub dag_history_depth: u32,
    pub node_name: Option<String>,
    pub metrics_port: u16,
    pub quorum_threshold: usize,
}

impl AimpConfig {
    pub fn new() -> Result<Self, ConfigError> {
        let s = Config::builder()
            // 1. Defaults
            .set_default("port", 1337)?
            .set_default("network_log_capacity", 500)?
            .set_default("max_visible_logs", 20)?
            .set_default("dag_history_depth", 100)?
            .set_default("metrics_port", 9090)?
            .set_default("quorum_threshold", 2)?
            // 2. Load from file
            .add_source(File::with_name("aimp.toml").required(false))
            // 3. Load from Environment Variables (e.g. AIMP_PORT=8080)
            .add_source(Environment::with_prefix("AIMP"))
            .build()?;

        s.try_deserialize()
    }
}

// Protocol & Tuning
pub const PROTOCOL_VERSION: u8 = 1;
pub const GC_MUTATION_THRESHOLD: u64 = 1000;
pub const GOSSIP_LRU_SIZE: usize = 1000;
pub const NETWORK_BACKPRESSURE_LIMIT: usize = 100;
pub const NETWORK_BUFFER_SIZE: usize = 65507; // UDP Max payload
pub const PEER_FAILURE_THRESHOLD: u32 = 5;

// Legacy constants for backward compatibility during transition
pub const DEFAULT_PORT: u16 = 1337;
pub const NETWORK_LOG_CAPACITY: usize = 500;
pub const MAX_VISIBLE_LOGS: usize = 20;
pub const DAG_HISTORY_DEPTH: u32 = 100;

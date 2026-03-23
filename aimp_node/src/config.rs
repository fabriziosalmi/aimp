use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

/// Dynamic configuration for an AIMP mesh node.
///
/// Loaded from (in priority order): CLI arguments > environment variables (`AIMP_` prefix) >
/// `aimp.toml` file > hardcoded defaults.
#[derive(Debug, Deserialize, Clone)]
pub struct AimpConfig {
    pub port: u16,
    pub network_log_capacity: usize,
    pub max_visible_logs: usize,
    pub dag_history_depth: u32,
    pub node_name: Option<String>,
    pub metrics_port: u16,
    pub quorum_threshold: usize,
    pub gc_mutation_threshold: u64,
    pub noise_required: bool,
    pub peer_rate_limit: u64,
    pub peer_rate_burst: u64,
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
            .set_default("gc_mutation_threshold", 1000)?
            .set_default("noise_required", true)?
            .set_default("peer_rate_limit", 50)?
            .set_default("peer_rate_burst", 100)?
            // 2. Load from file
            .add_source(File::with_name("aimp.toml").required(false))
            // 3. Load from Environment Variables (e.g. AIMP_PORT=8080)
            .add_source(Environment::with_prefix("AIMP"))
            .build()?;

        let cfg: Self = s.try_deserialize()?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Validate configuration invariants. Returns an error on invalid combinations.
    fn validate(&self) -> Result<(), ConfigError> {
        if self.quorum_threshold == 0 {
            return Err(ConfigError::Message("quorum_threshold must be > 0".into()));
        }
        if self.dag_history_depth == 0 {
            return Err(ConfigError::Message("dag_history_depth must be > 0".into()));
        }
        if self.gc_mutation_threshold == 0 {
            return Err(ConfigError::Message(
                "gc_mutation_threshold must be > 0".into(),
            ));
        }
        if self.peer_rate_limit == 0 {
            return Err(ConfigError::Message("peer_rate_limit must be > 0".into()));
        }
        if self.peer_rate_burst == 0 {
            return Err(ConfigError::Message("peer_rate_burst must be > 0".into()));
        }
        Ok(())
    }
}

// Protocol & Tuning
pub const PROTOCOL_VERSION: u8 = 1;
/// Minimum protocol version accepted (supports rolling upgrades).
pub const MIN_PROTOCOL_VERSION: u8 = 1;
pub const GC_MUTATION_THRESHOLD: u64 = 1000;
pub const GOSSIP_LRU_SIZE: usize = 1000;
pub const NETWORK_BACKPRESSURE_LIMIT: usize = 100;
pub const NETWORK_BUFFER_SIZE: usize = 65507; // UDP Max payload
pub const PEER_FAILURE_THRESHOLD: u32 = 5;
pub const SESSION_MAX_COUNT: usize = 256;
pub const SESSION_TTL_SECS: u64 = 300; // 5 minutes

// Legacy constants for backward compatibility during transition
pub const DEFAULT_PORT: u16 = 1337;
pub const NETWORK_LOG_CAPACITY: usize = 500;
pub const MAX_VISIBLE_LOGS: usize = 20;
pub const DAG_HISTORY_DEPTH: u32 = 100;

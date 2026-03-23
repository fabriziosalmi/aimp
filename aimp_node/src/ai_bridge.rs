use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::AimpResult;
use crate::event::SystemEvent;

/// Output of a deterministic AI inference.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct AiDecision {
    /// The entity targeted by this decision.
    pub target_entity: String,
    /// Status classification (e.g. "CRITICAL", "NORMAL").
    pub status: String,
    /// Whether the decision requires operator action.
    pub action_required: bool,
}

/// Trait for pluggable inference engines.
///
/// All implementations must be **deterministic**: given the same input,
/// they must produce the exact same output across all mesh nodes.
pub trait InferenceEngine: Send + Sync {
    /// Run inference on the given prompt and return a decision.
    fn infer(&self, prompt: &str) -> AimpResult<AiDecision>;

    /// Return a deterministic hash identifying this engine's logic version.
    fn model_hash(&self) -> [u8; 32];
}

/// A single rule for the rule-based inference engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRule {
    /// Keywords that trigger this rule (matched case-insensitively).
    pub keywords: Vec<String>,
    /// The target entity to assign when triggered.
    pub target: String,
    /// The status to assign when triggered.
    pub status: String,
    /// Whether action is required.
    pub action_required: bool,
}

/// Rule-based inference engine with configurable rules.
///
/// Rules are evaluated in order; the first matching rule wins.
/// If no rule matches, a default "NORMAL" decision is returned.
/// Rules can be loaded from a JSON file and hot-reloaded at runtime.
pub struct RuleEngine {
    rules: Vec<InferenceRule>,
    version: String,
}

impl RuleEngine {
    /// Create a rule engine with the given rules and version identifier.
    pub fn new(rules: Vec<InferenceRule>, version: String) -> Self {
        Self { rules, version }
    }

    /// Load rules from a JSON file. Returns `None` if the file doesn't exist or is invalid.
    ///
    /// Expected format: `[{"keywords": [...], "target": "...", "status": "...", "action_required": bool}, ...]`
    pub fn from_file(path: &std::path::Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        let rules: Vec<InferenceRule> = serde_json::from_str(&content).ok()?;
        let version = format!(
            "rules.file.{}",
            crate::crypto::SecurityFirewall::hash(content.as_bytes())
                .iter()
                .take(4)
                .map(|b| format!("{:02x}", b))
                .collect::<String>()
        );
        Some(Self::new(rules, version))
    }

    /// Create a rule engine with the default built-in rules.
    pub fn default_rules() -> Self {
        let rules = vec![
            InferenceRule {
                keywords: vec![
                    "error".into(),
                    "failure".into(),
                    "fault".into(),
                    "critical".into(),
                    "danger".into(),
                ],
                target: "system_alert".into(),
                status: "CRITICAL".into(),
                action_required: true,
            },
            InferenceRule {
                keywords: vec!["valve".into(), "pressure".into(), "flow".into()],
                target: "hydraulic_system".into(),
                status: "WARNING".into(),
                action_required: true,
            },
            InferenceRule {
                keywords: vec!["north".into(), "nord".into()],
                target: "sector_north".into(),
                status: "NORMAL".into(),
                action_required: false,
            },
            InferenceRule {
                keywords: vec!["south".into(), "sud".into()],
                target: "sector_south".into(),
                status: "NORMAL".into(),
                action_required: false,
            },
        ];
        Self::new(rules, "rules.v2.default".into())
    }
}

impl InferenceEngine for RuleEngine {
    fn infer(&self, prompt: &str) -> AimpResult<AiDecision> {
        let prompt_lower = prompt.to_lowercase();

        for rule in &self.rules {
            if rule.keywords.iter().any(|kw| prompt_lower.contains(kw)) {
                return Ok(AiDecision {
                    target_entity: rule.target.clone(),
                    status: rule.status.clone(),
                    action_required: rule.action_required,
                });
            }
        }

        Ok(AiDecision {
            target_entity: "generic_entity".into(),
            status: "NORMAL".into(),
            action_required: false,
        })
    }

    fn model_hash(&self) -> [u8; 32] {
        crate::crypto::SecurityFirewall::hash(self.version.as_bytes())
    }
}

/// Main AI engine dispatcher.
///
/// Wraps an `InferenceEngine` implementation and provides logging,
/// hot-reload from an optional rules file, and integration with the AIMP event system.
pub struct AiEngine {
    engine: std::sync::RwLock<Box<dyn InferenceEngine>>,
    rules_path: Option<std::path::PathBuf>,
    rules_hash: std::sync::Mutex<Option<[u8; 32]>>,
    log_tx: Option<mpsc::Sender<SystemEvent>>,
}

impl AiEngine {
    /// Create a new AI engine. If `aimp_rules.json` exists, load rules from it;
    /// otherwise use the default built-in rules.
    pub fn new(log_tx: Option<mpsc::Sender<SystemEvent>>) -> AimpResult<Self> {
        let rules_path = std::path::PathBuf::from("aimp_rules.json");
        let (engine, hash): (Box<dyn InferenceEngine>, Option<[u8; 32]>) =
            if let Some(re) = RuleEngine::from_file(&rules_path) {
                let h = re.model_hash();
                (Box::new(re), Some(h))
            } else {
                (Box::new(RuleEngine::default_rules()), None)
            };

        Ok(Self {
            engine: std::sync::RwLock::new(engine),
            rules_path: Some(rules_path),
            rules_hash: std::sync::Mutex::new(hash),
            log_tx,
        })
    }

    /// Check if the rules file has changed and reload if needed.
    fn try_hot_reload(&self) {
        let path = match &self.rules_path {
            Some(p) if p.exists() => p,
            _ => return,
        };

        if let Some(new_engine) = RuleEngine::from_file(path) {
            let new_hash = new_engine.model_hash();
            let mut current_hash = self.rules_hash.lock().unwrap();
            if *current_hash == Some(new_hash) {
                return; // No change
            }
            *current_hash = Some(new_hash);
            drop(current_hash);

            if let Ok(mut engine) = self.engine.write() {
                *engine = Box::new(new_engine);
            }
            if let Some(ref tx) = self.log_tx {
                let _ = tx.try_send(SystemEvent::Status(
                    "[AI] Rules hot-reloaded from aimp_rules.json".into(),
                ));
            }
        }
    }

    /// Create an AI engine with a custom inference engine.
    pub fn with_engine(
        engine: Box<dyn InferenceEngine>,
        log_tx: Option<mpsc::Sender<SystemEvent>>,
    ) -> Self {
        Self {
            engine: std::sync::RwLock::new(engine),
            rules_path: None,
            rules_hash: std::sync::Mutex::new(None),
            log_tx,
        }
    }

    /// Run deterministic inference and log the result.
    ///
    /// Checks for rule file changes before each inference (hot-reload).
    /// Guarantees identical output across all mesh nodes given the same input.
    pub async fn run_deterministic_inference(
        &self,
        mesh_prompt: &str,
        _crdt_context: &str,
    ) -> AimpResult<AiDecision> {
        self.try_hot_reload();

        let decision = self.engine.read().unwrap().infer(mesh_prompt)?;

        if let Some(ref tx) = self.log_tx {
            let _ = tx.try_send(SystemEvent::AiInference {
                prompt: mesh_prompt.to_string(),
                decision: format!("{:?}", decision),
            });
        }

        Ok(decision)
    }

    /// Return the model hash of the active inference engine.
    pub fn get_model_hash(&self) -> [u8; 32] {
        self.engine.read().unwrap().model_hash()
    }
}

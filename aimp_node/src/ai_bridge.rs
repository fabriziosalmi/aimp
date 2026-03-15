use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

// ==========================================
// DETERMINISTIC AI BRIDGE
// ==========================================
// This module implements a local-first, zero-dependency 
// deterministic heuristic engine to replace external LLMs.

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct AiDecision {
    pub target_entity: String, 
    pub status: String,        
    pub action_required: bool, 
}

use crate::event::SystemEvent;

pub struct AiEngine {
    capabilities: Vec<String>,
    log_tx: Option<mpsc::Sender<SystemEvent>>,
}

impl AiEngine {
    pub fn new(log_tx: Option<mpsc::Sender<SystemEvent>>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(AiEngine {
            capabilities: vec!["logic.deterministic".to_string(), "safety.critical".to_string()],
            log_tx,
        })
    }

    pub fn get_capabilities(&self) -> Vec<String> {
        self.capabilities.clone()
    }

    /// Pure Rust Deterministic Inference (High-Reliability Fallback)
    /// Guarantees identical output across all mesh nodes given the same input.
    pub async fn run_deterministic_inference(
        &self, 
        mesh_prompt: &str, 
        _crdt_context: &str
    ) -> Result<AiDecision, String> {
        
        let prompt_upper = mesh_prompt.to_uppercase();
        
        // 1. DETERMINISTIC HEURISTIC LOGIC
        // In a production aerospace environment, this would be a verified DFA or Rule-Engine.
        let is_critical = prompt_upper.contains("ERRORE") 
            || prompt_upper.contains("VALVOLA") 
            || prompt_upper.contains("PRESSIONE")
            || prompt_upper.contains("PERICOLO");

        let target = if prompt_upper.contains("NORD") {
            "valvola_nord"
        } else if prompt_upper.contains("SUD") {
            "valvola_sud"
        } else {
            "generic_entity"
        };

        let decision = AiDecision {
            target_entity: target.to_string(),
            status: if is_critical { "CRITICO".to_string() } else { "NORMALE".to_string() },
            action_required: is_critical,
        };

        if let Some(ref tx) = self.log_tx {
            let _ = tx.try_send(SystemEvent::AiInference { 
                prompt: mesh_prompt.to_string(), 
                decision: format!("{:?}", decision) 
            });
        }
        
        // v0.4.0: Return decision along with model metadata for evidence
        Ok(decision)
    }

    pub fn get_model_hash(&self) -> [u8; 32] {
        // Deterministic hash of the heuristic logic version
        crate::crypto::SecurityFirewall::hash(b"heuristic.v1.deterministic")
    }
}

use crate::config;
use crate::crypto::Identity;
use snow::{Builder, HandshakeState, TransportState};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// State machine for a Noise Protocol XX session with a single peer.
pub enum SecureState {
    Handshaking(Box<HandshakeState>),
    Active(TransportState),
    Invalid,
}

/// A Noise Protocol XX session with a specific peer, tracking handshake or transport state.
pub struct SecureSession {
    pub state: SecureState,
    pub peer_addr: SocketAddr,
    /// Last time this session was used (for LRU eviction).
    pub last_used: Instant,
}

/// Manages Noise Protocol XX sessions for all connected peers.
///
/// Sessions are evicted when the count exceeds `SESSION_MAX_COUNT` or
/// when a session has been idle longer than `SESSION_TTL_SECS`.
pub struct SessionManager {
    identity: Arc<Identity>,
    sessions: Arc<RwLock<HashMap<SocketAddr, SecureSession>>>,
}

impl SessionManager {
    pub fn new(identity: Arc<Identity>) -> Self {
        Self {
            identity,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Evict sessions that are expired or over capacity.
    /// Must be called while holding the write lock (sessions passed by ref).
    fn evict_stale(sessions: &mut HashMap<SocketAddr, SecureSession>) {
        let now = Instant::now();
        let ttl = std::time::Duration::from_secs(config::SESSION_TTL_SECS);

        // Remove expired sessions and invalid sessions
        sessions.retain(|_, s| {
            now.duration_since(s.last_used) < ttl && !matches!(s.state, SecureState::Invalid)
        });

        // If still over capacity, remove oldest sessions
        while sessions.len() > config::SESSION_MAX_COUNT {
            let oldest = sessions
                .iter()
                .min_by_key(|(_, s)| s.last_used)
                .map(|(addr, _)| *addr);

            if let Some(addr) = oldest {
                sessions.remove(&addr);
            } else {
                break;
            }
        }
    }

    /// Prepares an encrypted message for a peer.
    /// If no session exists, it returns a handshake initiator message.
    pub async fn wrap(&self, peer: SocketAddr, payload: &[u8]) -> (Vec<u8>, bool) {
        let mut sessions = self.sessions.write().await;

        Self::evict_stale(&mut sessions);

        let session = sessions
            .entry(peer)
            .or_insert_with(|| SecureSession::new_initiator(&self.identity, peer));

        session.last_used = Instant::now();

        match &mut session.state {
            SecureState::Active(transport) => {
                let mut buf = vec![0u8; payload.len() + 16];
                if let Ok(n) = transport.write_message(payload, &mut buf) {
                    buf.truncate(n);
                    return (buf, false);
                }
                (vec![], false)
            }
            SecureState::Handshaking(handshake) => {
                let mut buf = vec![0u8; 1024];
                if let Ok(n) = handshake.write_message(&[], &mut buf) {
                    buf.truncate(n);
                    return (buf, true);
                }
                (vec![], true)
            }
            SecureState::Invalid => (vec![], false),
        }
    }

    /// Processes an incoming message from a peer.
    /// Returns Some(decrypted_payload) if it was a data message or a successful handshake.
    pub async fn unwrap(&self, peer: SocketAddr, payload: &[u8]) -> Option<Vec<u8>> {
        let mut sessions = self.sessions.write().await;

        Self::evict_stale(&mut sessions);

        let session = sessions
            .entry(peer)
            .or_insert_with(|| SecureSession::new_responder(&self.identity, peer));

        session.last_used = Instant::now();

        match &mut session.state {
            SecureState::Handshaking(handshake) => {
                let mut buf = vec![0u8; 1024];
                if let Ok(n) = handshake.read_message(payload, &mut buf) {
                    if handshake.is_handshake_finished() {
                        if let SecureState::Handshaking(hs) =
                            std::mem::replace(&mut session.state, SecureState::Invalid)
                        {
                            if let Ok(transport) = (*hs).into_transport_mode() {
                                session.state = SecureState::Active(transport);
                            }
                        }
                    }
                    return Some(buf[..n].to_vec());
                }
                None
            }
            SecureState::Active(transport) => {
                let mut buf = vec![0u8; payload.len()];
                if let Ok(n) = transport.read_message(payload, &mut buf) {
                    buf.truncate(n);
                    return Some(buf);
                }
                None
            }
            SecureState::Invalid => None,
        }
    }
}

impl SecureSession {
    pub fn new_initiator(identity: &Identity, peer_addr: SocketAddr) -> Self {
        let builder = Builder::new("Noise_XX_25519_ChaChaPoly_BLAKE3".parse().unwrap());
        let static_key = identity.noise_static_secret.to_bytes();
        let handshake = builder
            .local_private_key(&static_key)
            .build_initiator()
            .unwrap();

        Self {
            state: SecureState::Handshaking(Box::new(handshake)),
            peer_addr,
            last_used: Instant::now(),
        }
    }

    pub fn new_responder(identity: &Identity, peer_addr: SocketAddr) -> Self {
        let builder = Builder::new("Noise_XX_25519_ChaChaPoly_BLAKE3".parse().unwrap());
        let static_key = identity.noise_static_secret.to_bytes();
        let handshake = builder
            .local_private_key(&static_key)
            .build_responder()
            .unwrap();

        Self {
            state: SecureState::Handshaking(Box::new(handshake)),
            peer_addr,
            last_used: Instant::now(),
        }
    }
}

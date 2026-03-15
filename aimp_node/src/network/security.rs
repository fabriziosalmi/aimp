use snow::{Builder, HandshakeState, TransportState};
use crate::crypto::Identity;
use std::net::SocketAddr;
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::Arc;

pub enum SecureState {
    Handshaking(HandshakeState),
    Active(TransportState),
    Invalid,
}

pub struct SecureSession {
    pub state: SecureState,
    pub peer_addr: SocketAddr,
}

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

    /// Prepares an encrypted message for a peer.
    /// If no session exists, it returns a handshake initiator message.
    pub async fn wrap(&self, peer: SocketAddr, payload: &[u8]) -> (Vec<u8>, bool) {
        let mut sessions = self.sessions.write().await;
        
        let session = sessions.entry(peer).or_insert_with(|| {
            SecureSession::new_initiator(&self.identity, peer)
        });

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
        
        let session = sessions.entry(peer).or_insert_with(|| {
            SecureSession::new_responder(&self.identity, peer)
        });

        match &mut session.state {
            SecureState::Handshaking(handshake) => {
                let mut buf = vec![0u8; 1024];
                if let Ok(n) = handshake.read_message(payload, &mut buf) {
                    if handshake.is_handshake_finished() {
                        // Transition to Active mode
                        if let SecureState::Handshaking(hs) = std::mem::replace(&mut session.state, SecureState::Invalid) {
                            let transport = hs.into_transport_mode().unwrap();
                            session.state = SecureState::Active(transport);
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
            state: SecureState::Handshaking(handshake),
            peer_addr,
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
            state: SecureState::Handshaking(handshake),
            peer_addr,
        }
    }
}

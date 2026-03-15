pub mod security;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Semaphore;
use tokio::net::UdpSocket;
use dashmap::DashMap;

use crate::protocol::{AimpEnvelope, AimpData, ProtocolParser};
use crate::crypto::{SecurityFirewall, Identity};
use crate::config;
use crate::network::security::SessionManager;

// ==========================================
// 1. GOSSIP ENGINE (AIMP v2 Legacy Bridge)
// ==========================================

pub struct GossipNetwork {
    socket: Arc<UdpSocket>,           
    port: u16,                        
    seen_messages: Vec<[u8; 64]>,      // Bounded ring buffer for signature tracking
    log_tx: Option<mpsc::Sender<crate::event::SystemEvent>>,
    identity: Arc<Identity>,
    backpressure: Arc<Semaphore>,
    peer_health: Arc<DashMap<String, u32>>, // Peer IP -> Failure Count
    security: Arc<SessionManager>,
}

impl GossipNetwork {
    pub async fn new(port: u16, identity: Arc<Identity>, log_tx: Option<mpsc::Sender<crate::event::SystemEvent>>) -> Result<Self, Box<dyn std::error::Error>> {
        let addr = format!("0.0.0.0:{}", port);
        let socket = UdpSocket::bind(&addr).await?; 
        socket.set_broadcast(true)?;

        Ok(GossipNetwork {
            socket: Arc::new(socket),
            port,
            seen_messages: Vec::with_capacity(config::GOSSIP_LRU_SIZE),
            log_tx,
            identity: identity.clone(),
            backpressure: Arc::new(Semaphore::new(config::NETWORK_BACKPRESSURE_LIMIT)),
            peer_health: Arc::new(DashMap::new()),
            security: Arc::new(SessionManager::new(identity)),
        })
    }

    pub fn clone_for_tx(&self) -> Self {
        GossipNetwork {
            socket: self.socket.clone(),
            port: self.port,
            seen_messages: Vec::new(),
            log_tx: self.log_tx.clone(),
            identity: self.identity.clone(),
            backpressure: self.backpressure.clone(),
            peer_health: self.peer_health.clone(),
            security: self.security.clone(),
        }
    }

    // ==========================================
    // 2. RECEIVER LOOP (Non-blocking)
    // ==========================================

    pub async fn listen(&mut self, tx: mpsc::Sender<(AimpEnvelope, SocketAddr)>) {
        let mut buf = vec![0u8; config::NETWORK_BUFFER_SIZE]; 

        loop {
            // SOTA Pattern: Backpressure. Wait for a permit before receiving more.
            let permit = self.backpressure.clone().acquire_owned().await.unwrap_or_else(|_| panic!("Semaphore closed"));

            let result = self.socket.recv_from(&mut buf).await;
            
            match result {
                Ok((len, peer_addr)) => {
                    let peer_ip = peer_addr.ip().to_string();

                    // Circuit Breaker check
                    if let Some(fail_count) = self.peer_health.get(&peer_ip) {
                        if *fail_count >= config::PEER_FAILURE_THRESHOLD {
                            continue; // Drop packets from bad peers
                        }
                    }

                    let raw_bytes = &buf[..len];
                    
                    // SOTA Security: Noise Protocol Unwrap
                    let decrypted = self.security.unwrap(peer_addr, raw_bytes).await;
                    if decrypted.is_none() {
                        continue; // Handshake packet or invalid encrypted data handled internally
                    }
                    let plain_bytes = decrypted.unwrap();

                    let envelope = match ProtocolParser::from_bytes(&plain_bytes) {
                        Ok(env) => env,
                        Err(_e) => {
                            let mut entry = self.peer_health.entry(peer_ip).or_insert(0);
                            *entry += 1;
                            continue;
                        }
                    };

                    // Gossip filter (Bounded)
                    if self.seen_messages.contains(&envelope.signature) {
                        continue; 
                    }

                    // Zero-Trust Firewall (Ed25519)
                    if !SecurityFirewall::verify(&envelope) {
                        let mut entry = self.peer_health.entry(peer_ip.clone()).or_insert(0);
                        *entry += 1;

                        if let Some(ref tx) = self.log_tx {
                            let _ = tx.try_send(crate::event::SystemEvent::SecurityDrop { 
                                peer: peer_ip, 
                                reason: "Circuit Breaker: Invalid Sig".into() 
                            });
                        }
                        continue;
                    }

                    self.seen_messages.push(envelope.signature);
                    if self.seen_messages.len() > config::GOSSIP_LRU_SIZE {
                        self.seen_messages.remove(0);
                    }

                    let tx_inner = tx.clone();
                    let log_tx_inner = self.log_tx.clone();
                    
                    // Release permit when task finishes
                    tokio::spawn(async move {
                        let _permit = permit; 
                        if let Err(e) = tx_inner.send((envelope, peer_addr)).await {
                            if let Some(ref log) = log_tx_inner {
                                let _ = log.try_send(crate::event::SystemEvent::Status(format!("Task Error: {}", e)));
                            }
                        }
                    });
                }
                Err(e) => eprintln!("Socket error: {}", e),
            }
        }
    }

    // ==========================================
    // 3. BROADCAST (Aerospace-grade signed)
    // ==========================================

    pub async fn broadcast(&mut self, mut data: AimpData) -> Result<(), Box<dyn std::error::Error>> {
        if data.ttl == 0 {
            return Ok(());
        }
        data.ttl -= 1;

        let signed_envelope = self.identity.sign(data)?;
        let bytes_to_send = ProtocolParser::to_bytes(&signed_envelope)?;

        self.seen_messages.push(signed_envelope.signature);
        if self.seen_messages.len() > config::GOSSIP_LRU_SIZE {
            self.seen_messages.remove(0);
        }

        let broadcast_addr: SocketAddr = format!("255.255.255.255:{}", self.port).parse().unwrap();
        
        let (encrypted_to_send, _is_handshake) = self.security.wrap(broadcast_addr, &bytes_to_send).await;
        
        if !encrypted_to_send.is_empty() {
             self.socket.send_to(&encrypted_to_send, broadcast_addr).await?;
        }

        Ok(())
    }
}

pub mod security;

use dashmap::DashMap;
use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::sync::Semaphore;

use crate::config;
use crate::crypto::{Identity, SecurityFirewall};
use crate::error::AimpResult;
use crate::network::security::SessionManager;
use crate::protocol::{AimpData, AimpEnvelope, ProtocolParser};

/// Per-peer rate limiting state using an integer-based token bucket.
///
/// Uses millisecond arithmetic to avoid floating-point precision loss.
/// Tokens are refilled based on elapsed time since last refill.
struct RateBucket {
    tokens: u64,
    last_refill: Instant,
}

/// O(1) bounded deduplication filter for gossip messages.
///
/// Uses a `VecDeque` as a FIFO ring buffer for eviction order and a `HashSet`
/// for O(1) membership checks, replacing the previous O(n) `Vec::contains`.
struct GossipFilter {
    queue: VecDeque<[u8; 64]>,
    set: HashSet<[u8; 64]>,
    capacity: usize,
}

impl GossipFilter {
    fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::with_capacity(capacity),
            set: HashSet::with_capacity(capacity),
            capacity,
        }
    }

    /// Returns true if the signature was already seen.
    fn contains(&self, sig: &[u8; 64]) -> bool {
        self.set.contains(sig)
    }

    /// Insert a signature. Evicts the oldest entry if at capacity.
    fn insert(&mut self, sig: [u8; 64]) {
        if self.set.contains(&sig) {
            return;
        }
        if self.queue.len() >= self.capacity {
            if let Some(old) = self.queue.pop_front() {
                self.set.remove(&old);
            }
        }
        self.queue.push_back(sig);
        self.set.insert(sig);
    }
}

/// UDP gossip network with zero-trust verification, backpressure, circuit breaker,
/// per-peer rate limiting, and optional Noise Protocol encryption.
pub struct GossipNetwork {
    socket: Arc<UdpSocket>,
    port: u16,
    gossip_filter: GossipFilter,
    log_tx: Option<mpsc::Sender<crate::event::SystemEvent>>,
    identity: Arc<Identity>,
    backpressure: Arc<Semaphore>,
    peer_health: Arc<DashMap<String, u32>>,
    security: Arc<SessionManager>,
    rate_limits: Arc<DashMap<String, RateBucket>>,
    peer_rate_limit: u64,
    peer_rate_burst: u64,
    noise_required: bool,
}

impl GossipNetwork {
    pub async fn new(
        port: u16,
        identity: Arc<Identity>,
        log_tx: Option<mpsc::Sender<crate::event::SystemEvent>>,
        noise_required: bool,
        peer_rate_limit: u64,
        peer_rate_burst: u64,
    ) -> AimpResult<Self> {
        let addr = format!("0.0.0.0:{}", port);
        let socket = UdpSocket::bind(&addr).await?;
        socket.set_broadcast(true)?;

        Ok(GossipNetwork {
            socket: Arc::new(socket),
            port,
            gossip_filter: GossipFilter::new(config::GOSSIP_LRU_SIZE),
            log_tx,
            identity: identity.clone(),
            backpressure: Arc::new(Semaphore::new(config::NETWORK_BACKPRESSURE_LIMIT)),
            peer_health: Arc::new(DashMap::new()),
            security: Arc::new(SessionManager::new(identity)),
            rate_limits: Arc::new(DashMap::new()),
            peer_rate_limit,
            peer_rate_burst,
            noise_required,
        })
    }

    /// Integer-based token bucket rate limiter.
    ///
    /// Uses millisecond precision with integer arithmetic to avoid
    /// floating-point drift. `peer_rate_limit` is tokens/sec,
    /// `peer_rate_burst` is max bucket capacity.
    fn check_rate_limit(&self, peer_ip: &str) -> bool {
        let now = Instant::now();

        let mut bucket = self
            .rate_limits
            .entry(peer_ip.to_string())
            .or_insert_with(|| RateBucket {
                tokens: self.peer_rate_burst,
                last_refill: now,
            });

        // Refill tokens: elapsed_ms * rate / 1000
        let elapsed_ms = now.duration_since(bucket.last_refill).as_millis() as u64;
        let refill = elapsed_ms * self.peer_rate_limit / 1000;
        if refill > 0 {
            bucket.tokens = (bucket.tokens + refill).min(self.peer_rate_burst);
            bucket.last_refill = now;
        }

        if bucket.tokens > 0 {
            bucket.tokens -= 1;
            true
        } else {
            false
        }
    }

    pub fn clone_for_tx(&self) -> Self {
        GossipNetwork {
            socket: self.socket.clone(),
            port: self.port,
            gossip_filter: GossipFilter::new(config::GOSSIP_LRU_SIZE),
            log_tx: self.log_tx.clone(),
            identity: self.identity.clone(),
            backpressure: self.backpressure.clone(),
            peer_health: self.peer_health.clone(),
            security: self.security.clone(),
            rate_limits: self.rate_limits.clone(),
            peer_rate_limit: self.peer_rate_limit,
            peer_rate_burst: self.peer_rate_burst,
            noise_required: self.noise_required,
        }
    }

    /// Non-blocking receiver loop with backpressure, rate limiting, and zero-trust verification.
    pub async fn listen(&mut self, tx: mpsc::Sender<(AimpEnvelope, SocketAddr)>) {
        let mut buf = vec![0u8; config::NETWORK_BUFFER_SIZE];

        loop {
            let permit = match self.backpressure.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => {
                    if let Some(ref log) = self.log_tx {
                        let _ = log.try_send(crate::event::SystemEvent::Status(
                            "Backpressure semaphore closed, stopping listener".into(),
                        ));
                    }
                    break;
                }
            };

            let result = self.socket.recv_from(&mut buf).await;

            match result {
                Ok((len, peer_addr)) => {
                    let peer_ip = peer_addr.ip().to_string();

                    // Per-peer rate limiting (checked before expensive crypto ops)
                    if !self.check_rate_limit(&peer_ip) {
                        if let Some(ref tx) = self.log_tx {
                            let _ = tx.try_send(crate::event::SystemEvent::SecurityDrop {
                                peer: peer_ip,
                                reason: "Rate limit exceeded".into(),
                            });
                        }
                        continue;
                    }

                    // Circuit Breaker check
                    if let Some(fail_count) = self.peer_health.get(&peer_ip) {
                        if *fail_count >= config::PEER_FAILURE_THRESHOLD {
                            continue;
                        }
                    }

                    let raw_bytes = &buf[..len];

                    // Noise Protocol: unwrap or pass through based on config
                    let plain_bytes = if self.noise_required {
                        match self.security.unwrap(peer_addr, raw_bytes).await {
                            Some(b) => b,
                            None => continue,
                        }
                    } else {
                        match self.security.unwrap(peer_addr, raw_bytes).await {
                            Some(b) => b,
                            None => raw_bytes.to_vec(),
                        }
                    };

                    let envelope = match ProtocolParser::from_bytes(&plain_bytes) {
                        Ok(env) => env,
                        Err(_e) => {
                            let mut entry = self.peer_health.entry(peer_ip).or_insert(0);
                            *entry += 1;
                            continue;
                        }
                    };

                    // TTL sanity check: messages arriving with TTL=0 after decrement
                    // should not exist on the wire. Count them as suspicious.
                    if envelope.data.ttl == 0 {
                        let mut entry = self.peer_health.entry(peer_ip.clone()).or_insert(0);
                        *entry += 1;
                        if let Some(ref tx) = self.log_tx {
                            let _ = tx.try_send(crate::event::SystemEvent::SecurityDrop {
                                peer: peer_ip,
                                reason: "TTL exhausted on arrival".into(),
                            });
                        }
                        continue;
                    }

                    // Gossip dedup filter — O(1) lookup via HashSet
                    if self.gossip_filter.contains(&envelope.signature) {
                        continue;
                    }

                    // Zero-Trust Firewall (Ed25519)
                    if !SecurityFirewall::verify(&envelope) {
                        let mut entry = self.peer_health.entry(peer_ip.clone()).or_insert(0);
                        *entry += 1;

                        if let Some(ref tx) = self.log_tx {
                            let _ = tx.try_send(crate::event::SystemEvent::SecurityDrop {
                                peer: peer_ip,
                                reason: "Circuit Breaker: Invalid Sig".into(),
                            });
                        }
                        continue;
                    }

                    self.gossip_filter.insert(envelope.signature);

                    let tx_inner = tx.clone();
                    let log_tx_inner = self.log_tx.clone();

                    tokio::spawn(async move {
                        let _permit = permit;
                        if let Err(e) = tx_inner.send((envelope, peer_addr)).await {
                            if let Some(ref log) = log_tx_inner {
                                let _ = log.try_send(crate::event::SystemEvent::Status(format!(
                                    "Task Error: {}",
                                    e
                                )));
                            }
                        }
                    });
                }
                Err(e) => eprintln!("Socket error: {}", e),
            }
        }
    }

    /// Broadcast a signed message to all peers via UDP.
    pub async fn broadcast(&mut self, mut data: AimpData) -> AimpResult<()> {
        if data.ttl == 0 {
            return Ok(());
        }
        data.ttl -= 1;

        let signed_envelope = self.identity.sign(data)?;
        let bytes_to_send = ProtocolParser::to_bytes(&signed_envelope)?;

        self.gossip_filter.insert(signed_envelope.signature);

        let broadcast_addr: SocketAddr = format!("255.255.255.255:{}", self.port).parse().unwrap();

        let (encrypted_to_send, _is_handshake) =
            self.security.wrap(broadcast_addr, &bytes_to_send).await;

        if !encrypted_to_send.is_empty() {
            self.socket
                .send_to(&encrypted_to_send, broadcast_addr)
                .await?;
        }

        Ok(())
    }
}

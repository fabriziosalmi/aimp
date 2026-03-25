use once_cell::sync::Lazy;
use prometheus::{Counter, Gauge, Histogram, HistogramOpts, Opts, Registry};

/// Prometheus metrics for the AIMP node.
///
/// Exposed via the `/metrics` HTTP endpoint for scraping by Prometheus or compatible collectors.
/// Includes counters, gauges, and latency histograms for comprehensive observability.
pub struct Metrics {
    /// Prometheus registry holding all registered metrics.
    pub registry: Registry,
    /// Total number of mutations processed (monotonic counter).
    pub mutation_count: Counter,
    /// Number of currently active gossip peers (gauge).
    pub peer_count: Gauge,
    /// Total number of nodes in the Merkle-DAG (gauge).
    pub dag_size: Gauge,
    /// Latency histogram for state sync operations (seconds).
    pub sync_duration: Histogram,
    /// Latency histogram for AI inference operations (seconds).
    pub inference_duration: Histogram,
    /// Latency histogram for Ed25519 signature verification (seconds).
    pub verify_duration: Histogram,
}

/// Global metrics instance, lazily initialized on first access.
pub static GLOBAL_METRICS: Lazy<Metrics> = Lazy::new(|| {
    let registry = Registry::new();

    let mutation_count = Counter::with_opts(Opts::new(
        "aimp_mutations_total",
        "Total mutations processed",
    ))
    .unwrap();
    let peer_count = Gauge::with_opts(Opts::new(
        "aimp_peers_active",
        "Number of active gossip peers",
    ))
    .unwrap();
    let dag_size = Gauge::with_opts(Opts::new(
        "aimp_dag_nodes_total",
        "Total nodes in the Merkle-DAG",
    ))
    .unwrap();

    // Latency histograms with buckets tuned for sub-millisecond to multi-second operations
    let fast_buckets = vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0];
    let slow_buckets = vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0];

    let sync_duration = Histogram::with_opts(
        HistogramOpts::new("aimp_sync_duration_seconds", "State sync operation latency")
            .buckets(slow_buckets.clone()),
    )
    .unwrap();

    let inference_duration = Histogram::with_opts(
        HistogramOpts::new(
            "aimp_inference_duration_seconds",
            "AI inference operation latency",
        )
        .buckets(slow_buckets),
    )
    .unwrap();

    let verify_duration = Histogram::with_opts(
        HistogramOpts::new(
            "aimp_verify_duration_seconds",
            "Ed25519 signature verification latency",
        )
        .buckets(fast_buckets),
    )
    .unwrap();

    registry.register(Box::new(mutation_count.clone())).unwrap();
    registry.register(Box::new(peer_count.clone())).unwrap();
    registry.register(Box::new(dag_size.clone())).unwrap();
    registry.register(Box::new(sync_duration.clone())).unwrap();
    registry
        .register(Box::new(inference_duration.clone()))
        .unwrap();
    registry
        .register(Box::new(verify_duration.clone()))
        .unwrap();

    Metrics {
        registry,
        mutation_count,
        peer_count,
        dag_size,
        sync_duration,
        inference_duration,
        verify_duration,
    }
});

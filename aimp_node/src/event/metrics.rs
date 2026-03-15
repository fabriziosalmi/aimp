use prometheus::{Counter, Gauge, Registry, Opts};
use once_cell::sync::Lazy;

pub struct Metrics {
    pub registry: Registry,
    pub mutation_count: Counter,
    pub peer_count: Gauge,
    pub dag_size: Gauge,
}

pub static GLOBAL_METRICS: Lazy<Metrics> = Lazy::new(|| {
    let registry = Registry::new();
    
    let mutation_count = Counter::with_opts(Opts::new("aimp_mutations_total", "Total mutations processed")).unwrap();
    let peer_count = Gauge::with_opts(Opts::new("aimp_peers_active", "Number of active gossip peers")).unwrap();
    let dag_size = Gauge::with_opts(Opts::new("aimp_dag_nodes_total", "Total nodes in the Merkle-DAG")).unwrap();

    registry.register(Box::new(mutation_count.clone())).unwrap();
    registry.register(Box::new(peer_count.clone())).unwrap();
    registry.register(Box::new(dag_size.clone())).unwrap();

    Metrics {
        registry,
        mutation_count,
        peer_count,
        dag_size,
    }
});

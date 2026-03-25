#!/usr/bin/env bash
# AIMP Reproducible Benchmark Suite
#
# Runs all benchmarks and saves structured results for paper verification.
#
# Usage:
#   ./benchmarks/run_all.sh              # Default (dalek backend)
#   ./benchmarks/run_all.sh --fast       # ring + mimalloc + target-cpu=native
#
# Prerequisites:
#   - Rust 1.75+ (rustup default stable)
#   - Docker (optional, for ARM64 constrained benchmarks)
#
# Output:
#   benchmarks/results/           — JSON results + markdown report
#   benchmarks/results/report.md  — Human-readable summary

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RESULTS_DIR="$SCRIPT_DIR/results"
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

# Parse flags
FEATURES=""
RUSTFLAGS_EXTRA=""
MODE="standard"

if [[ "${1:-}" == "--fast" ]]; then
    FEATURES="--features fast-crypto,fast-alloc"
    RUSTFLAGS_EXTRA="-C target-cpu=native"
    MODE="fast"
fi

mkdir -p "$RESULTS_DIR"

echo "================================================================"
echo "AIMP Reproducible Benchmark Suite"
echo "================================================================"
echo "Mode:      $MODE"
echo "Features:  ${FEATURES:-none}"
echo "RUSTFLAGS: ${RUSTFLAGS_EXTRA:-none}"
echo "Timestamp: $TIMESTAMP"
echo "Platform:  $(uname -m) / $(uname -s)"
echo "Rust:      $(rustc --version)"
echo "================================================================"
echo ""

# Save environment info
cat > "$RESULTS_DIR/environment.json" << ENVEOF
{
    "timestamp": "$TIMESTAMP",
    "mode": "$MODE",
    "platform": "$(uname -m)",
    "os": "$(uname -s) $(uname -r)",
    "rust_version": "$(rustc --version)",
    "cargo_version": "$(cargo --version)",
    "cpu": "$(sysctl -n machdep.cpu.brand_string 2>/dev/null || cat /proc/cpuinfo 2>/dev/null | grep 'model name' | head -1 | cut -d: -f2 | xargs || echo 'unknown')",
    "features": "${FEATURES:-none}",
    "rustflags": "${RUSTFLAGS_EXTRA:-none}"
}
ENVEOF

cd "$PROJECT_DIR"

# ---- Build ----
echo "[1/6] Building release binaries..."
RUSTFLAGS="$RUSTFLAGS_EXTRA" cargo build --release --manifest-path aimp_node/Cargo.toml \
    $FEATURES \
    --example bench_convergence \
    --example bench_netem \
    --example profile_mutation \
    2>&1 | tail -3

# ---- Tests ----
echo ""
echo "[2/6] Running test suite..."
cargo test --manifest-path aimp_node/Cargo.toml $FEATURES 2>&1 | tail -5
echo ""

# ---- Criterion micro-benchmarks ----
echo "[3/6] Running Criterion micro-benchmarks..."
RUSTFLAGS="$RUSTFLAGS_EXTRA" cargo bench --manifest-path aimp_node/Cargo.toml 2>&1 | \
    grep -E "^(append_mutation|merkle_root|blake3|serialize|deserialize|ed25519)" | \
    tee "$RESULTS_DIR/criterion_raw.txt"
echo ""

# ---- System convergence ----
echo "[4/6] Running system convergence benchmark..."
RUSTFLAGS="$RUSTFLAGS_EXTRA" cargo run --release --manifest-path aimp_node/Cargo.toml \
    $FEATURES --example bench_convergence 2>&1 | \
    tee "$RESULTS_DIR/convergence_raw.txt"
echo ""

# ---- Network impairment ----
echo "[5/6] Running network impairment benchmark..."
RUSTFLAGS="$RUSTFLAGS_EXTRA" cargo run --release --manifest-path aimp_node/Cargo.toml \
    $FEATURES --example bench_netem 2>&1 | \
    tee "$RESULTS_DIR/netem_raw.txt"
echo ""

# ---- Hot-path profile ----
echo "[6/6] Running hot-path profiler..."
RUSTFLAGS="$RUSTFLAGS_EXTRA" cargo run --release --manifest-path aimp_node/Cargo.toml \
    $FEATURES --example profile_mutation 2>&1 | \
    tee "$RESULTS_DIR/profile_raw.txt"
echo ""

# ---- Automerge comparison (only if benchmarks crate exists) ----
if cargo metadata --manifest-path benchmarks/Cargo.toml --no-deps >/dev/null 2>&1; then
    echo "[BONUS] Running Automerge comparison..."
    RUSTFLAGS="$RUSTFLAGS_EXTRA" cargo run --release -p aimp_comparison_bench 2>&1 | \
        tee "$RESULTS_DIR/automerge_raw.txt"
    echo ""
fi

# ---- Docker ARM64 (only if Docker is available) ----
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    echo "[BONUS] Running ARM64 constrained benchmark (Docker)..."
    docker build -f Dockerfile.bench --platform linux/arm64 -t aimp-bench-arm64 . 2>&1 | tail -3
    docker run --rm --platform linux/arm64 --memory=1g --cpus=1 \
        aimp-bench-arm64 bench_convergence 2>&1 | \
        tee "$RESULTS_DIR/arm64_constrained_raw.txt"
    echo ""
fi

# ---- Generate report ----
echo "================================================================"
echo "BENCHMARK COMPLETE — Results in $RESULTS_DIR/"
echo "================================================================"
ls -la "$RESULTS_DIR/"
echo ""
echo "To reproduce:"
echo "  git clone https://github.com/fabriziosalmi/aimp.git"
echo "  cd aimp"
echo "  ./benchmarks/run_all.sh         # standard"
echo "  ./benchmarks/run_all.sh --fast  # ring + mimalloc + native"

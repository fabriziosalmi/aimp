#!/bin/bash
# Run all L3 Epistemic Layer benchmarks — mirrors benchmarks/run_all.sh for L2
#
# Usage: ./benchmarks/run_epistemic.sh
# Results: benchmarks/results/epistemic_*

set -euo pipefail

RESULTS_DIR="benchmarks/results"
mkdir -p "$RESULTS_DIR"

echo "================================================"
echo " AIMP L3 Epistemic Layer — Full Benchmark Suite"
echo "================================================"
echo ""

# 1. Criterion micro-benchmarks
echo "=== [1/6] Criterion Micro-benchmarks ==="
cargo bench --bench epistemic -- --save-baseline l3 2>&1 | tee "$RESULTS_DIR/epistemic_criterion_raw.txt"
echo ""

# 2. Hot-path profiling
echo "=== [2/6] Hot-Path Profiling ==="
cargo run --release --example profile_epistemic 2>&1 | tee "$RESULTS_DIR/epistemic_hotpath_raw.txt"
echo ""

# 3. Scalability benchmarks
echo "=== [3/6] Scalability Benchmarks ==="
cargo run --release --example bench_belief_scale 2>&1 | tee "$RESULTS_DIR/epistemic_scale_raw.txt"
echo ""

# 4. SOTA comparison
echo "=== [4/6] SOTA Comparison (Subjective Logic, Dempster-Shafer) ==="
cargo run --release --example compare_subjective_logic 2>&1 | tee "$RESULTS_DIR/subjective_logic_comparison.txt"
echo ""

# 5. P4 revocation benchmark
echo "=== [5/6] P4 Credential Revocation Benchmark ==="
cargo run --release --example bench_revocation 2>&1 | tee "$RESULTS_DIR/revocation_raw.txt"
echo ""

# 6. Property-based tests
echo "=== [6/6] Property-Based Tests (proptest, 256 cases) ==="
cargo test --test epistemic_proptests -- --test-threads=1 2>&1 | tee "$RESULTS_DIR/proptest_summary.txt"
echo ""

echo "================================================"
echo " All L3 benchmarks complete."
echo " Results saved to: $RESULTS_DIR/epistemic_*"
echo "================================================"

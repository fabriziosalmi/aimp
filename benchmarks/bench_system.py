#!/usr/bin/env python3
"""
AIMP System-Level Benchmark Harness

Measures:
1. End-to-end throughput: mutations/sec with N nodes
2. Convergence time: time for all nodes to reach same merkle root after mutations
3. Partition/merge: convergence after simulated network partition

Requirements:
  pip install msgpack pynacl requests

Usage:
  # Start the cluster first:
  cd benchmarks && docker compose up -d --build

  # Run benchmarks:
  python3 benchmarks/bench_system.py
"""

import json
import socket
import struct
import sys
import time
from collections import OrderedDict
from typing import Optional

import msgpack
import requests
from nacl.signing import SigningKey

# Node addresses
NODES = {
    "node1": {"udp": ("127.0.0.1", 1337), "http": "http://127.0.0.1:9091", "mesh_ip": "172.30.0.11"},
    "node2": {"udp": ("127.0.0.1", 1337), "http": "http://127.0.0.1:9092", "mesh_ip": "172.30.0.12"},
    "node3": {"udp": ("127.0.0.1", 1337), "http": "http://127.0.0.1:9093", "mesh_ip": "172.30.0.13"},
    "node4": {"udp": ("127.0.0.1", 1337), "http": "http://127.0.0.1:9094", "mesh_ip": "172.30.0.14"},
    "node5": {"udp": ("127.0.0.1", 1337), "http": "http://127.0.0.1:9095", "mesh_ip": "172.30.0.15"},
}


class AimpBenchClient:
    """Minimal AIMP client for benchmarking — sends signed UDP envelopes."""

    # OpCode constants
    OP_INFER = 0x04

    def __init__(self):
        self.signing_key = SigningKey.generate()
        self.verify_key = self.signing_key.verify_key
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.clock = 0

    def _build_envelope(self, op: int, payload: bytes) -> bytes:
        """Build a signed AIMP envelope as MessagePack."""
        self.clock += 1
        pubkey = bytes(self.verify_key)

        data = OrderedDict([
            ("v", 1),
            ("op", op),
            ("ttl", 3),
            ("origin_pubkey", pubkey),
            ("vclock", {"bench": self.clock}),
            ("payload", payload),
        ])

        data_bytes = msgpack.packb(data, use_bin_type=True)
        signed = self.signing_key.sign(data_bytes)
        signature = signed.signature  # 64 bytes

        envelope = OrderedDict([
            ("data", data),
            ("signature", signature),
        ])

        return msgpack.packb(envelope, use_bin_type=True)

    def send_infer(self, target: tuple, prompt: str):
        """Send an inference request to a node."""
        payload = msgpack.packb(prompt, use_bin_type=True)
        envelope = self._build_envelope(self.OP_INFER, payload)
        self.sock.sendto(envelope, target)

    def close(self):
        self.sock.close()


def get_merkle_root(node_name: str) -> Optional[str]:
    """Get merkle root from a node's health endpoint."""
    try:
        resp = requests.get(f"{NODES[node_name]['http']}/health", timeout=2)
        data = resp.json()
        return data.get("checks", {}).get("crdt", {}).get("merkle_root")
    except Exception:
        return None


def get_all_roots() -> dict:
    """Get merkle roots from all nodes."""
    roots = {}
    for name in NODES:
        root = get_merkle_root(name)
        if root:
            roots[name] = root
    return roots


def check_convergence(roots: dict) -> bool:
    """Check if all nodes have converged to the same non-zero root."""
    values = list(roots.values())
    if len(values) < len(NODES):
        return False
    zero_root = "0" * 64
    return len(set(values)) == 1 and values[0] != zero_root


def wait_for_nodes(timeout: float = 30.0):
    """Wait until all nodes are healthy."""
    print(f"Waiting for {len(NODES)} nodes to come online...")
    start = time.time()
    while time.time() - start < timeout:
        healthy = 0
        for name in NODES:
            try:
                resp = requests.get(f"{NODES[name]['http']}/health", timeout=1)
                if resp.status_code == 200:
                    healthy += 1
            except Exception:
                pass
        if healthy == len(NODES):
            print(f"All {len(NODES)} nodes online ({time.time() - start:.1f}s)")
            return True
        time.sleep(1)
    print(f"ERROR: Only {healthy}/{len(NODES)} nodes came online after {timeout}s")
    return False


# ---------------------------------------------------------------------------
# Benchmark 1: Throughput — mutations/sec to a single node
# ---------------------------------------------------------------------------
def bench_throughput(num_mutations: int = 1000):
    print(f"\n{'='*60}")
    print(f"BENCHMARK 1: Throughput ({num_mutations} mutations to node1)")
    print(f"{'='*60}")

    client = AimpBenchClient()
    target = NODES["node1"]["udp"]

    start = time.time()
    for i in range(num_mutations):
        client.send_infer(target, f"bench mutation {i}")
    elapsed = time.time() - start

    rate = num_mutations / elapsed
    print(f"  Sent {num_mutations} mutations in {elapsed:.3f}s")
    print(f"  Throughput: {rate:.0f} msg/sec (client-side send rate)")

    client.close()
    return {"mutations": num_mutations, "elapsed_s": elapsed, "rate_msg_sec": rate}


# ---------------------------------------------------------------------------
# Benchmark 2: Convergence — time for N nodes to agree on merkle root
# ---------------------------------------------------------------------------
def bench_convergence(num_mutations: int = 100, timeout: float = 30.0):
    print(f"\n{'='*60}")
    print(f"BENCHMARK 2: Convergence ({num_mutations} mutations, {len(NODES)} nodes)")
    print(f"{'='*60}")

    # Get initial roots
    initial_roots = get_all_roots()
    print(f"  Initial roots: {len(set(initial_roots.values()))} distinct values")

    # Send mutations to node1 only
    client = AimpBenchClient()
    target = NODES["node1"]["udp"]

    send_start = time.time()
    for i in range(num_mutations):
        client.send_infer(target, f"convergence test {i}")
    send_elapsed = time.time() - send_start
    print(f"  Sent {num_mutations} mutations in {send_elapsed:.3f}s")
    client.close()

    # Poll for convergence
    converge_start = time.time()
    converged = False
    poll_count = 0

    while time.time() - converge_start < timeout:
        roots = get_all_roots()
        poll_count += 1
        if check_convergence(roots):
            converge_elapsed = time.time() - converge_start
            total_elapsed = time.time() - send_start
            print(f"  CONVERGED in {converge_elapsed:.3f}s (total: {total_elapsed:.3f}s)")
            print(f"  Final root: {list(roots.values())[0][:16]}...")
            print(f"  Polls: {poll_count}")
            converged = True
            return {
                "mutations": num_mutations,
                "nodes": len(NODES),
                "converge_time_s": converge_elapsed,
                "total_time_s": total_elapsed,
                "converged": True,
            }
        time.sleep(0.1)

    roots = get_all_roots()
    distinct = len(set(roots.values()))
    print(f"  TIMEOUT after {timeout}s — {distinct} distinct roots across {len(roots)} nodes")
    for name, root in roots.items():
        print(f"    {name}: {root[:16]}...")
    return {
        "mutations": num_mutations,
        "nodes": len(NODES),
        "converge_time_s": timeout,
        "total_time_s": timeout + send_elapsed,
        "converged": False,
        "distinct_roots": distinct,
    }


# ---------------------------------------------------------------------------
# Benchmark 3: Node health / latency
# ---------------------------------------------------------------------------
def bench_health_latency(rounds: int = 50):
    print(f"\n{'='*60}")
    print(f"BENCHMARK 3: Health endpoint latency ({rounds} rounds)")
    print(f"{'='*60}")

    results = {}
    for name in NODES:
        latencies = []
        for _ in range(rounds):
            start = time.time()
            try:
                resp = requests.get(f"{NODES[name]['http']}/health", timeout=2)
                latencies.append((time.time() - start) * 1000)  # ms
            except Exception:
                pass

        if latencies:
            latencies.sort()
            p50 = latencies[len(latencies) // 2]
            p99 = latencies[int(len(latencies) * 0.99)]
            results[name] = {"p50_ms": p50, "p99_ms": p99, "samples": len(latencies)}
            print(f"  {name}: p50={p50:.1f}ms  p99={p99:.1f}ms  ({len(latencies)} samples)")

    return results


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main():
    print("AIMP System Benchmark Harness")
    print(f"Nodes: {len(NODES)}")
    print()

    if not wait_for_nodes():
        sys.exit(1)

    results = {}

    # Give nodes a moment to fully initialize
    time.sleep(2)

    results["throughput"] = bench_throughput(1000)
    time.sleep(2)

    results["convergence"] = bench_convergence(100)
    time.sleep(2)

    results["health_latency"] = bench_health_latency(50)

    # Summary
    print(f"\n{'='*60}")
    print("SUMMARY")
    print(f"{'='*60}")
    print(json.dumps(results, indent=2, default=str))

    # Write results to file
    with open("benchmarks/results.json", "w") as f:
        json.dump(results, f, indent=2, default=str)
    print(f"\nResults written to benchmarks/results.json")


if __name__ == "__main__":
    main()

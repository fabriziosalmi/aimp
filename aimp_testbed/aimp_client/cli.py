"""AIMP CLI — Command-line tool for interacting with AIMP mesh nodes.

Usage:
    aimp-cli infer "Check valve pressure" --target 127.0.0.1 --port 1337
    aimp-cli ping --target 127.0.0.1 --port 1337
    aimp-cli health --target 127.0.0.1 --metrics-port 9090
    aimp-cli metrics --target 127.0.0.1 --metrics-port 9090
"""

import argparse
import json
import sys
import urllib.request

from aimp_client.client import AimpClient


def cmd_infer(args):
    """Send an inference request to the mesh."""
    with AimpClient(target=args.target, port=args.port) as client:
        sent = client.send_infer(args.prompt)
        print(f"Sent {sent} bytes to {args.target}:{args.port}")
        print(f"Prompt: {args.prompt}")
        print(f"Node ID: {client.identity.pubkey_hex[:16]}...")


def cmd_ping(args):
    """Send a gossip ping."""
    with AimpClient(target=args.target, port=args.port) as client:
        sent = client.send_ping()
        print(f"Ping sent ({sent} bytes) to {args.target}:{args.port}")
        print(f"Node ID: {client.identity.pubkey_hex[:16]}...")


def cmd_health(args):
    """Query the node health endpoint."""
    url = f"http://{args.target}:{args.metrics_port}/health"
    try:
        with urllib.request.urlopen(url, timeout=5) as resp:
            data = json.loads(resp.read())
            healthy = data.get("healthy", False)
            status = "HEALTHY" if healthy else "UNHEALTHY"
            print(f"Node: {args.target}:{args.metrics_port} — {status}")
            for check, detail in data.get("checks", {}).items():
                s = detail.get("status", "unknown")
                extra = ""
                if "merkle_root" in detail:
                    extra = f" root={detail['merkle_root'][:16]}..."
                if "available" in detail:
                    extra = f" capacity={detail['available']}"
                print(f"  {check}: {s}{extra}")
    except Exception as e:
        print(f"ERROR: Could not reach {url}: {e}", file=sys.stderr)
        sys.exit(1)


def cmd_metrics(args):
    """Fetch raw Prometheus metrics."""
    url = f"http://{args.target}:{args.metrics_port}/metrics"
    try:
        with urllib.request.urlopen(url, timeout=5) as resp:
            print(resp.read().decode())
    except Exception as e:
        print(f"ERROR: Could not reach {url}: {e}", file=sys.stderr)
        sys.exit(1)


def main():
    parser = argparse.ArgumentParser(
        prog="aimp-cli",
        description="AIMP Mesh Protocol CLI",
    )
    parser.add_argument("--target", default="127.0.0.1", help="Node IP address")
    parser.add_argument("--port", type=int, default=1337, help="Node UDP port")
    parser.add_argument("--metrics-port", type=int, default=9090, help="Metrics HTTP port")

    sub = parser.add_subparsers(dest="command", required=True)

    p_infer = sub.add_parser("infer", help="Send an AI inference request")
    p_infer.add_argument("prompt", help="Inference prompt text")
    p_infer.set_defaults(func=cmd_infer)

    p_ping = sub.add_parser("ping", help="Send a gossip ping")
    p_ping.set_defaults(func=cmd_ping)

    p_health = sub.add_parser("health", help="Query node health")
    p_health.set_defaults(func=cmd_health)

    p_metrics = sub.add_parser("metrics", help="Fetch Prometheus metrics")
    p_metrics.set_defaults(func=cmd_metrics)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()

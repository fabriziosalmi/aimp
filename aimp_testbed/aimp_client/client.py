"""Core AIMP client implementation."""

import socket
from enum import IntEnum
from typing import Optional

import msgpack
import nacl.signing


class OpCode(IntEnum):
    """AIMP protocol operation codes."""
    PING = 0x01
    SYNC_REQ = 0x02
    SYNC_RES = 0x03
    INFER = 0x04


class AimpIdentity:
    """Ed25519 cryptographic identity for an AIMP node.

    Manages key generation, signing, and public key export.
    """

    def __init__(self, signing_key: Optional[nacl.signing.SigningKey] = None):
        """Create a new identity, or wrap an existing signing key."""
        self._signing_key = signing_key or nacl.signing.SigningKey.generate()
        self._verify_key = self._signing_key.verify_key

    @property
    def pubkey(self) -> bytes:
        """Return the 32-byte Ed25519 public key."""
        return bytes(self._verify_key.encode())

    @property
    def pubkey_hex(self) -> str:
        """Return the hex-encoded public key."""
        return self.pubkey.hex()

    def sign(self, data: bytes) -> bytes:
        """Sign data and return the 64-byte signature."""
        signed = self._signing_key.sign(data)
        return signed.signature

    @classmethod
    def from_seed(cls, seed: bytes) -> "AimpIdentity":
        """Create an identity from a 32-byte seed (deterministic)."""
        return cls(nacl.signing.SigningKey(seed))


class AimpClient:
    """Client for sending messages to an AIMP mesh node.

    Handles identity, message construction, signing, and UDP transport.

    Example:
        client = AimpClient(target="127.0.0.1", port=1337)
        client.send_infer("Check sensor readings")
        client.send_ping()
    """

    PROTOCOL_VERSION = 1
    DEFAULT_TTL = 5

    def __init__(
        self,
        target: str = "127.0.0.1",
        port: int = 1337,
        identity: Optional[AimpIdentity] = None,
        ttl: int = DEFAULT_TTL,
    ):
        self.target = target
        self.port = port
        self.identity = identity or AimpIdentity()
        self.ttl = ttl
        self._vclock: dict[str, int] = {}
        self._tick = 0

        self._sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)

    def _next_vclock(self) -> dict[str, int]:
        """Increment and return the local vector clock."""
        self._tick += 1
        node_prefix = self.identity.pubkey_hex[:8]
        self._vclock[node_prefix] = self._tick
        return dict(self._vclock)

    def _build_envelope(self, op: OpCode, payload: bytes) -> bytes:
        """Build a signed AIMP envelope and return MessagePack bytes."""
        data = [
            self.PROTOCOL_VERSION,
            int(op),
            self.ttl,
            self.identity.pubkey,
            self._next_vclock(),
            payload,
        ]
        data_bytes = msgpack.packb(data, use_bin_type=True)
        signature = self.identity.sign(data_bytes)
        envelope = [data, signature]
        return msgpack.packb(envelope, use_bin_type=True)

    def send_raw(self, op: OpCode, payload: bytes) -> int:
        """Send a raw AIMP message. Returns bytes sent."""
        packet = self._build_envelope(op, payload)
        return self._sock.sendto(packet, (self.target, self.port))

    def send_infer(self, prompt: str) -> int:
        """Send an AI inference request to the mesh."""
        return self.send_raw(OpCode.INFER, prompt.encode("utf-8"))

    def send_ping(self, merkle_root: Optional[bytes] = None) -> int:
        """Send a gossip ping with the local Merkle root."""
        root = merkle_root or b"\x00" * 32
        return self.send_raw(OpCode.PING, root)

    def send_sync_request(self, heads: list[bytes]) -> int:
        """Request delta sync by sending local head hashes."""
        payload = msgpack.packb(heads, use_bin_type=True)
        return self.send_raw(OpCode.SYNC_REQ, payload)

    def close(self):
        """Close the UDP socket."""
        self._sock.close()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

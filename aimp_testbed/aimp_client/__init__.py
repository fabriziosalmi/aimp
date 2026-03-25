"""AIMP Client SDK — Python client for the AI Mesh Protocol.

Provides identity management, message construction, signing, and
UDP transport for interacting with AIMP mesh nodes.

Usage:
    from aimp_client import AimpClient, OpCode

    client = AimpClient(target="127.0.0.1", port=1337)
    client.send_infer("Check valve pressure status")
    client.send_ping()
"""

from aimp_client.client import AimpClient, OpCode, AimpIdentity

__all__ = ["AimpClient", "OpCode", "AimpIdentity"]
__version__ = "0.1.0"

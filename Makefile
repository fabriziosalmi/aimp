# AIMP Build & Deployment Makefile
#
# Targets:
#   make build              — Debug build (native)
#   make release            — Release build (native)
#   make edge-arm64         — Static musl binary for ARM64 edge devices
#   make edge-armv7         — Static musl binary for ARMv7 (Raspberry Pi, etc.)
#   make edge-x86           — Static musl binary for x86_64 edge gateways
#   make edge-all           — All edge targets
#   make test               — Run all tests
#   make bench              — Run benchmarks
#   make lint               — Format + clippy
#   make docs               — Generate rustdoc
#   make microvm-rootfs     — Build minimal rootfs for Firecracker
#   make clean              — Clean build artifacts

.PHONY: build release test bench lint docs clean \
        edge-arm64 edge-armv7 edge-x86 edge-all \
        microvm-rootfs install-cross-targets

CARGO = cargo
BINARY = aimp_node
VERSION = $(shell grep '^version' aimp_node/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
DIST_DIR = dist

# ─── Native Builds ───────────────────────────────────────────────

build:
	$(CARGO) build

release:
	$(CARGO) build --release

test:
	$(CARGO) test

bench:
	$(CARGO) bench

lint:
	$(CARGO) fmt --check
	$(CARGO) clippy -- -D warnings

docs:
	$(CARGO) doc --manifest-path aimp_node/Cargo.toml --no-deps

# ─── Cross-Compilation for Edge ──────────────────────────────────
#
# Prerequisites:
#   rustup target add aarch64-unknown-linux-musl
#   rustup target add armv7-unknown-linux-musleabihf
#   rustup target add x86_64-unknown-linux-musl
#
# For ARM cross-compilation on macOS/x86, install cross-compilers:
#   brew install filosottile/musl-cross/musl-cross  (macOS)
#   apt install gcc-aarch64-linux-gnu               (Ubuntu)
#
# Or use `cross` (Docker-based, handles toolchains automatically):
#   cargo install cross
#   Replace $(CARGO) with cross in the targets below.

install-cross-targets:
	rustup target add aarch64-unknown-linux-musl
	rustup target add armv7-unknown-linux-musleabihf
	rustup target add x86_64-unknown-linux-musl

edge-arm64:
	@mkdir -p $(DIST_DIR)
	$(CARGO) build --release --target aarch64-unknown-linux-musl
	cp target/aarch64-unknown-linux-musl/release/$(BINARY) $(DIST_DIR)/$(BINARY)-$(VERSION)-aarch64-linux
	@echo "Built: $(DIST_DIR)/$(BINARY)-$(VERSION)-aarch64-linux"
	@ls -lh $(DIST_DIR)/$(BINARY)-$(VERSION)-aarch64-linux

edge-armv7:
	@mkdir -p $(DIST_DIR)
	$(CARGO) build --release --target armv7-unknown-linux-musleabihf
	cp target/armv7-unknown-linux-musleabihf/release/$(BINARY) $(DIST_DIR)/$(BINARY)-$(VERSION)-armv7-linux
	@echo "Built: $(DIST_DIR)/$(BINARY)-$(VERSION)-armv7-linux"
	@ls -lh $(DIST_DIR)/$(BINARY)-$(VERSION)-armv7-linux

edge-x86:
	@mkdir -p $(DIST_DIR)
	$(CARGO) build --release --target x86_64-unknown-linux-musl
	cp target/x86_64-unknown-linux-musl/release/$(BINARY) $(DIST_DIR)/$(BINARY)-$(VERSION)-x86_64-linux
	@echo "Built: $(DIST_DIR)/$(BINARY)-$(VERSION)-x86_64-linux"
	@ls -lh $(DIST_DIR)/$(BINARY)-$(VERSION)-x86_64-linux

edge-all: edge-arm64 edge-armv7 edge-x86
	@echo "All edge binaries in $(DIST_DIR)/"
	@ls -lh $(DIST_DIR)/

# ─── Firecracker MicroVM ─────────────────────────────────────────
#
# Builds a minimal Alpine rootfs (~15MB) with the AIMP binary inside.
# Requires: x86_64-unknown-linux-musl target (or aarch64 variant).
#
# To run with Firecracker:
#   firecracker --api-sock /tmp/fc.sock \
#     --config-file deploy/firecracker/vm-config.json

microvm-rootfs: edge-x86
	@echo "Building Firecracker rootfs..."
	deploy/firecracker/build-rootfs.sh $(DIST_DIR)/$(BINARY)-$(VERSION)-x86_64-linux
	@echo "Rootfs: $(DIST_DIR)/aimp-rootfs.ext4"

# ─── Cleanup ─────────────────────────────────────────────────────

clean:
	$(CARGO) clean
	rm -rf $(DIST_DIR)

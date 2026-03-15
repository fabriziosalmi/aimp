# Contributing to AIMP

Thank you for your interest in contributing to the AI Mesh Protocol (AIMP). We welcome contributors who prioritize engineering rigor and deterministic systems.

## Development Environment Setup

### Prerequisites
- **Rust**: >= 1.75.0
- **Python**: >= 3.10.0 (for testbed)
- **Protoc**: Required if you plan to modify serialization layers.

### Steps
1. Fork the repository.
2. Clone your fork locally.
3. Verify the build:
   ```bash
   cd aimp_node
   cargo check
   ```
4. Run the test suite:
   ```bash
   cargo test
   ```

## Contribution Process

1. **Open an Issue**: Discuss any significant changes before starting work.
2. **Feature Branch**: Create a branch with a descriptive name (`feat/protocol-versioning`).
3. **Commit Messages**: Use imperative voice (e.g., "Add circuit breaker for peer health").
4. **Testing**: All PRs must pass property-based tests.
5. **Static Analysis**: Ensure `cargo clippy` returns zero findings.

## Inclusive Language Guidelines
- Use "Folks/Team/Everyone" instead of gendered terms.
- Use "Main/Replica" or "Primary/Secondary" instead of "Master/Slave".
- Use "Validity check" instead of "Sanity check".

## Governance
This project is maintained by a core team of researchers and engineers. Decision-making is recorded via ADRs in `aimp_node/docs/adr/`.

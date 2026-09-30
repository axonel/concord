# Changelog

All notable changes to CONCORD will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.1] - 2026-09-21

### Added
- **Docker Compose Service Modeling**: Full lifecycle and state analysis of services declared in `docker-compose.yml`, `compose.yaml`, and compose variants.
- **Compose Blockers & Causal Tracing**: Detection of uninstantiable Compose stacks due to missing `.env` files or unresolved environment variable placeholders (`${VAR}`), with end-to-end causal chain explanation.
- **Container Observation & Health Tracking**: Live discovery of containers (`docker ps -a`) tracking ports, labels, exit codes, and health statuses (`Uncreated`, `Stopped`, `Unhealthy`, `Running`).
- **Strict Container-to-Service Isolation**: Compose project and service label matching (`com.docker.compose.project`, `com.docker.compose.service`) ensuring foreign host containers never satisfy project database requirements.
- **Multi-Component Architecture & Provenance Attribution**: Discovers monorepo subcomponents (`web/`, `mobile/`, `crates/*`, `packages/*`, `services/*`). Scored attribution strictly links runtime and tool requirements to their declaring components (e.g. Java pin strictly to `mobile`).
- **Typed Tool Taxonomy & Operational Scopes**: Distinguishes Runtimes, Package Managers, Developer Tools, Build Tools, and Code Generators across operational scopes (`RequiredForProject`, `RequiredForBuild`, `RequiredForTask`, `Optional`).
- **Set-Theoretic Multi-Source Constraint Consolidation**: Merges configuration signals (`mise.toml`, `package.json`, `.nvmrc`) via constraint intersection while preserving exact version pins (`==`).
- **Automated Integration Test Matrix**: 7 comprehensive test fixtures (A through G) validating host daemons, Compose services, env resolution, container states, foreign container isolation, and monorepo attribution.

### Fixed
- Fixed CI failure by programmatically generating self-contained `.env` fixtures with RAII cleanup, avoiding repository `.gitignore` masking.
- Fixed non-standard version normalization and multi-component vector comparison (e.g. OpenJDK `26.0.2.1`).

## [0.2.0] - 2026-09-21

### Added
- Multi-component project analysis.
- Rust Cargo and Go ecosystem detectors.
- Framework default port discovery (Next.js, Vite, Astro, FastAPI, Flask).
- 3-tier environment variable categorization (`Required`, `OptionalWithDefault`, `ConfiguredLocal`).
- Configuration conflict detection.
- Dynamic TCP socket probes.
- Graph traversal and root-cause causal diagnosis (`concord explain`).

## [0.1.1] - 2026-09-21

### Added
- First public installable binary release for Linux x86_64.
- One-command shell installer (`install.sh`) with SHA256 checksum verification.
- Automated release packaging and GitHub Actions pipeline.

## [0.1.0] - 2026-09-21

### Added
- Initial core architecture and deterministic Environment IR.
- Process, port, service, and runtime scanning.
- Basic constraint evaluation, failure prediction, and read-only verification.

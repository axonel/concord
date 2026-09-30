# CONCORD Implementation Plan — Phase 1 Engineering Foundation

## Core Principle
> **CONCORD must be useful without an LLM.** Any LLM integration is optional and sits strictly above deterministic system intelligence.

## Architecture
The system follows a clean pipeline moving from discovery to evidence-based diagnosis and verification:

```text
CLI (concord)
 ↓
Core (IR + Evidence Model + Confidence)
 ↓
Project Analyzer + Machine Scanner
 ↓
Environment IR
 ↓
Graph
 ↓
Constraint Engine
 ↓
Prediction / Diagnosis
 ↓
Verification
```

## Workspace Layout
```text
crates/
    core/         # Environment IR, Evidence, Confidence, common types & errors
    scanner/      # Deterministic Linux machine inspection with provenance
    project/      # Repository signal discovery & parsing (Node/Bun, Python, Docker, etc.)
    constraints/  # Data-driven constraint evaluation (Version, Port, Arch, OS, Memory)
    graph/        # Environment graph connecting project, capabilities, constraints, evidence
    predictor/    # Deterministic failure prediction
    diagnosis/    # Root-cause causal explanation without LLMs
    verifier/     # Read-only verification engine
    cli/          # Command-line interface with human & JSON formatting
tests/
    fixtures/     # Representative test repositories (healthy and broken)
docs/
    IMPLEMENTATION_PLAN.md
    ENVIRONMENT_MODEL.md
```

## Milestones

1. **Workspace Initialization**: Setup Cargo workspace, git repo, docs, `.gitignore`.
2. **Core Crate (`concord-core`)**: Domain models (`ProjectRequirement`, `MachineCapability`, `Runtime`, `Service`, `Port`, `EnvironmentModel`), `Evidence`, `Confidence`.
3. **Machine Scanner (`concord-scanner`)**: Linux OS/arch/CPU/memory/disk, PATH/env, runtimes (Node, Bun, Python, Rust, Go, Java), Docker, PostgreSQL, listening ports with process attribution.
4. **Project Analyzer (`concord-project`)**: Discovery for `package.json`, lockfiles, `pyproject.toml`, `requirements.txt`, `uv.lock`, `Dockerfile`, `compose.yaml`, `.nvmrc`, `.python-version`, `.env.example`.
5. **Constraint Engine (`concord-constraints`)**: Data-driven constraint definitions and evaluation without ad-hoc `if` branches.
6. **Environment Graph (`concord-graph`)**: Graph representation linking requirements, capabilities, constraints, and evidence.
7. **Prediction & Root-Cause Diagnosis (`concord-predictor`, `concord-diagnosis`)**: Deterministic prediction of failures (runtime version mismatches, port collisions, missing services) and causal chain explanation.
8. **Read-Only Verification (`concord-verifier`)**: Verification checks without machine modification.
9. **CLI Executable (`concord-cli`)**: `concord .`, `concord scan`, `concord predict`, `concord explain`, `concord verify`, `--json`.
10. **Fixtures & Tests**: Unit & integration tests against healthy and intentionally broken environments.
11. **Documentation & Polish**: README, ARCHITECTURE, ENVIRONMENT_MODEL.

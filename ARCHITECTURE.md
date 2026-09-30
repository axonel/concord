# CONCORD Architecture

This document describes the internal design, crate organization, data flow, and engineering principles of **CONCORD**.

---

## 1. System Pipeline

CONCORD processes environments through a unidirectional, deterministic pipeline:

```text
               CLI (concord)
                    │
        ┌───────────┴───────────┐
        ▼                       ▼
 Project Analyzer        Machine Scanner
 (concord-project)        (concord-scanner)
        │                       │
        └───────────┬───────────┘
                    ▼
             Environment IR
              (concord-core)
                    │
                    ▼
            Environment Graph
              (concord-graph)
                    │
                    ▼
            Constraint Engine
           (concord-constraints)
                    │
        ┌───────────┴───────────┐
        ▼                       ▼
Failure Predictor        Diagnosis Engine
(concord-predictor)       (concord-diagnosis)
        │                       │
        └───────────┬───────────┘
                    ▼
           Read-Only Verifier
           (concord-verifier)
                    │
                    ▼
          Presentation (CLI / JSON)
```

---

## 2. Crate Responsibilities

The codebase is organized into small, cohesive, decoupled crates:

| Crate | Responsibility | Dependencies |
| :--- | :--- | :--- |
| `concord-core` | Canonical Intermediate Representation (IR), `Evidence`, `EvidenceSource`, `Confidence`, and `ConcordError`. | `serde`, `semver`, `thiserror` |
| `concord-scanner` | Deterministic Linux machine inspection. Parses `/proc/net/tcp` for listening ports, probes `/proc/[pid]/fd` for process attribution, checks `/etc/os-release`, queries PATH for runtimes, and inspects Docker / PostgreSQL. | `concord-core`, `sysinfo`, `serde` |
| `concord-project` | Repository signal discovery. Parses `package.json`, `bun.lock`, `pyproject.toml`, `requirements.txt`, `Dockerfile`, `compose.yaml`, `.nvmrc`, `.python-version`, `.tool-versions`, and `.env.example`. | `concord-core`, `toml`, `serde_json`, `walkdir` |
| `concord-constraints` | Data-driven constraint evaluation (`RuntimeVersion`, `PortAvailable`, `ServiceRunning`, `MemoryMin`, `OsMatch`, `ArchMatch`, `EnvVarSet`). Evaluates constraints against machine state without ad-hoc `if` checks. | `concord-core`, `semver`, `serde` |
| `concord-graph` | Dependency and causality graph built on `petgraph`. Connects projects, requirements, capabilities, constraints, and evidence nodes to trace root causes. | `concord-core`, `concord-constraints`, `petgraph` |
| `concord-predictor` | Deterministic failure prediction. Identifies runtime incompatibilities, port collisions, missing services, and configuration gaps before execution. | `concord-core`, `concord-constraints`, `concord-graph` |
| `concord-diagnosis` | Root-cause analysis. Traces causal dependency chains back to the earliest violated invariant, producing structured explanations without LLMs. | `concord-core`, `concord-predictor`, `concord-graph` |
| `concord-verifier` | Non-destructive verification engine. Validates that project requirements, runtime versions, ports, and services are satisfied. | `concord-core`, `concord-constraints`, `concord-scanner`, `concord-project` |
| `concord` (CLI) | Top-level CLI binary and library. Implements `concord`, `scan`, `predict`, `explain`, `verify`, with human formatting and first-class JSON output. | All workspace crates, `clap`, `colored` |

---

## 3. Data Flow & Provenance

Every observation in CONCORD is backed by explicit evidence:

```text
Evidence
├── Source: RepositoryFile / ExecutableInspection / NetworkProbe / OsMetadata / ProcessInspection
├── Confidence: CONFIRMED / HIGH / MEDIUM / LOW / UNKNOWN
└── Description: Human-readable trace of the observation
```

### Invariant Evaluation

Instead of hardcoding checks like:
```rust
// ANTI-PATTERN: DO NOT DO THIS
if python_version < 3.11 {
    eprintln!("Python too old");
}
```

CONCORD translates project requirements into declarative `Constraint` records:
```rust
Constraint::RuntimeVersion {
    runtime: "python",
    constraint_str: ">= 3.11",
}
```

The evaluator matches the constraint against the `MachineCapability` model, yielding an `EvaluatedConstraint` that retains:
1. The evaluated constraint.
2. The evaluation status (`Satisfied`, `Violated`, `Unknown`).
3. Project evidence (e.g. `pyproject.toml: line 12`).
4. Machine evidence (e.g. `/usr/bin/python3 --version -> Python 3.10.12`).

---

## 4. Root-Cause Analysis & Causality

When a constraint is violated, `concord-graph` traverses the dependency graph to construct an ordered causal chain:

```text
Host machine state: Python 3.10.12 at /usr/bin/python3
       │
       ▼
Project specification: requires python >= 3.11
       │
       ▼
Violated invariant: python.version satisfies >= 3.11
       │
       ▼
Downstream impact: python toolchain cannot initialize; build and runtime will fail
```

This ensures that CONCORD explains **why** a project will fail at the earliest point of divergence, rather than merely reporting the symptom when a process crashes.

---

## 5. Architectural Roadmap

The Phase 1 foundation is designed to host future planned systems without architectural rework:

- **Repair Planner (Phase 2)**: DAG-based minimal repair planner.
- **Simulation Engine (Phase 2)**: Counterfactual environment simulation without machine mutation.
- **Transactional Execution & Rollback (Phase 2)**: Snapshots, filesystem transactions, and automated rollbacks.
- **Environment Reproducibility (Phase 3)**: Generation of `concord.lock`, Dockerfile, and devcontainer environments.
- **Drift Detection & Bisect (Phase 3)**: Historical failure database and git-aware regression bisecting.

# CONCORD Real-World Benchmarks & Evaluation

This document records the empirical performance, detection depth, and accuracy of **CONCORD v0.2.0** when evaluated against 6 production and active development repositories.

Testing environment:
- **OS**: Linux (Fedora Linux 43 x86_64, Kernel 6.19.7-200.fc43.x86_64)
- **CPU**: AMD Ryzen (16 cores)
- **Host Tools**: Node v26.8.1 (via mise), Bun 1.4.1, Python 3.14.7, Rust 1.87.0-nightly, Docker (active), PostgreSQL 18.6 (stopped)

---

## 1. Real Repository Evaluation Matrix

| Repository | Project Archetype | Languages Discovered | Discovered Components | Inferred Ports | Env Var Classification | Predicted Issues | Status | Release Execution Time |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`concord`** | Rust Cargo Workspace | Rust | 9 crates (`core`, `scanner`, `project`, `constraints`, `graph`, `predictor`, `diagnosis`, `verifier`, `cli`) | None | None | 0 (Clean) | `COMPATIBLE` | **~308 ms** |
| **`axonel`** | Polyglot Monorepo (Rust + Web) | Rust, TypeScript | 10 components (`web`, 9 Rust crates) | `5173` (Vite) | None | 0 (Clean) | `COMPATIBLE` | **~326 ms** |
| **`portfolio`** | Bun / TypeScript Frontend | Bun, TypeScript | Single root | `5173` (Vite) | None | 0 (Clean) | `COMPATIBLE` | **~294 ms** |
| **`child-ai`** | Node.js Backend & Web | TypeScript | 1 component (`backend`) | `3001` (config), `5173` (Vite) | 22 optional defaults | 0 (Clean) | `COMPATIBLE` | **~283 ms** |
| **`amux`** | Rust Terminal Multiplexer | Rust, TypeScript | 4 crates (`cli`, `core`, `dashboard`, `server`) | None | None | 0 (Clean) | `COMPATIBLE` | **~320 ms** |
| **`heym`** | Fullstack Polyglot (Python + Bun + Postgres + Docker) | Python, Bun, TypeScript | 2 components (`frontend`, `backend`) | `4017`, `5432`, `8000`, `10105` | 15 required, 42 optional defaults | 16 issues (Postgres stopped, 15 unset required secrets) | `16 PROBLEMS` | **~294 ms** |

---

## 2. Key Observations & Quality Advances

### 1. Eliminating False-Positive Alarms (`child-ai` and `heym`)
- **Before**: In v0.1.x, template `.env.example` files containing defaults (e.g. `PORT=3000`, `LOG_LEVEL=info`, `TIMEOUT=30`) were treated as strictly required environment variables missing from the developer's shell, generating 40+ false alarms on `heym` and 22 false alarms on `child-ai`.
- **After**: The 3-tier categorization (`Required`, `OptionalWithDefault`, `ConfiguredLocal`) cleanly isolates actual blockers (empty secrets like `API_SECRET_KEY=`) while recognizing fallback defaults. `child-ai` now passes cleanly with 0 false alarms.

### 2. Multi-Component Depth (`axonel`, `heym`, `amux`)
- **Before**: Root-only scanning missed nested frontend/backend components, submodules, and Cargo workspace crates.
- **After**: Subdirectories (`web/`, `frontend/`, `backend/`, `crates/*`, `packages/*`, `apps/*`) are modeled as first-class `ProjectComponent`s. Framework defaults (Vite 5173, Next.js 3000, FastAPI 8000) are automatically detected and verified against system port occupancy.

### 3. Causal Graph Diagnosis (`heym`)
- When `heym` failed on PostgreSQL dependency, running `concord explain --target postgresql` generated a complete graph causal chain:
  ```text
  1. Required service 'postgresql' failure predicted
     Root Cause:          service.postgresql.running
     Violated Constraint: Service 'postgresql' must be running
     Confidence:          HIGH
     Affected Components: postgresql, database/backend

     Causal Chain:
       ├─► Component 'heym' depends on service 'postgresql'
       ├─► Host machine daemon state: service postgresql (Stopped, version: Some("18.6"))
       ├─► First violated invariant: service postgresql is active and listening
       └─► Impact: connection attempts to postgresql will be refused
  ```

### 4. Dynamic Probes
- Service checks for PostgreSQL and Docker actively verify TCP responsiveness and daemon socket responsiveness with 300ms bounded non-blocking probes, rather than blindly assuming a running process implies an accepting socket.

### 5. Latency & Zero-AI Speed
- Across all evaluated real-world projects, full execution (scanning, parsing, probing, constraint checking, causal graph synthesis, and reporting) finishes in **under 350 ms**.

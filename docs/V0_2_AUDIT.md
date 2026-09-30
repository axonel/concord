# CONCORD v0.2.0 Architecture & Engineering Audit

**Date:** 2026-09-21  
**Scope:** Deep inspection of all workspace crates, tests, real-world repositories (`axonel`, `heym`, `portfolio`, `child-ai`, `amux`), and end-to-end resolution pipeline.

---

## Executive Summary

CONCORD v0.1.1 achieved the first clean-room installable distribution. However, an empirical audit against real-world repositories reveals that the internal engine is still largely shallow, produces excessive false positives on environment variables, misses entire language ecosystems (such as Rust and Go), ignores monorepo/multi-component project architectures, and generates synthetic causal chains instead of traversing a real environment graph.

To advance from *"CONCORD can be installed"* to *"CONCORD is actually useful"*, the engine must transition from naive keyword searches to rich, structured domain models backed by graph causal traversal and controlled dynamic probes.

---

## Detailed Component Audits

### 1. Project Analyzer (`concord-project`)

#### Audit Item 1.1: Missing Multi-Component & Monorepo Discovery
- **Current Capability:** Only inspects the repository root directory for `package.json`, `pyproject.toml`, `Dockerfile`, and `compose.yaml`.
- **Actual Limitation:** Completely misses frontend/backend subdirectories (e.g. `axonel/web`, `child-ai/backend`, `packages/*`, `apps/*`, `crates/*`).
- **Why It Matters:** On `axonel`, CONCORD reported 0 requirements and 0 languages, failing to detect both the Rust workspace and the React/Vite frontend in `web/`. On `child-ai`, it detected the frontend but was completely blind to the Node/Express API in `backend/`.
- **Proposed Engineering Change:** Implement a recursive / component-aware directory scanner that identifies nested project boundaries (`packages`, `apps`, `crates`, `web`, `backend`, `frontend`, `services`, `client`, `server`) and constructs a multi-component `ProjectManifest`.

#### Audit Item 1.2: Language Ecosystem Blind Spots (Rust, Go)
- **Current Capability:** Only detects Node/Bun and Python.
- **Actual Limitation:** Zero knowledge of Rust (`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `rust-toolchain`) or Go (`go.mod`, `go.sum`).
- **Why It Matters:** Running `concord .` on `axonel/concord` itself or `axonel/amux` resulted in zero detected requirements and missed compiler version constraints (e.g. `rust-version = "1.85"` or `rust-toolchain.toml`).
- **Proposed Engineering Change:** Add `rust.rs` analyzer detecting `Cargo.toml` (`[package].rust-version`, `[workspace]`, dependencies, binaries) and `rust-toolchain.toml`, plus `go.rs` analyzer for `go.mod`.

#### Audit Item 1.3: Superficial Node/Bun & Python Analysis
- **Current Capability:** Checks only `engines.node` in `package.json` and basic regex matching for `--port`.
- **Actual Limitation:** If `package.json` omits `engines`, Node requirement is reported as 0. Misses `@types/node` in `devDependencies`, default framework ports (Vite: 5173, Next.js: 3000, Remix: 3000, Astro: 4321, Express: 3000/8080), scripts analysis, and lockfile provenance.
- **Why It Matters:** On `portfolio` (Vite + React), CONCORD reported 0 requirements and missed port 5173 entirely.
- **Proposed Engineering Change:** Extract framework signals (Vite, Next, Nuxt, Remix, Astro, Express, Fastify) with inferred default ports and infer runtime bounds from `@types/node` and lockfiles.

#### Audit Item 1.4: Naive Docker & Compose Parsing
- **Current Capability:** Line-by-line regex looking for `EXPOSE ` and `image: *postgres*`.
- **Actual Limitation:** Ignores `FROM` base images (which declare runtimes like `node:20` or `python:3.11`), `ENV`, `ARG`, `WORKDIR`, `ENTRYPOINT`, `CMD`. Compose parser ignores `services`, `depends_on`, `environment`, `networks`, `volumes`, healthchecks, and non-standard Dockerfile names (`Dockerfile.*`).
- **Why It Matters:** In `amux`, Dockerfiles are named `Dockerfile.rust-base` and `Dockerfile.rust-build`, which were skipped completely. In compose files, database dependencies declared via `depends_on: [db]` and service ports are not mapped to components.
- **Proposed Engineering Change:** Parse Dockerfiles for `FROM`, `ENV`, `ARG`, `EXPOSE` and parse Compose YAML into structured `ComposeService` models with `image`, `ports`, `environment`, `depends_on`, and `healthcheck`.

---

### 2. Environment Variable Analysis (`concord-project` & `concord-constraints`)

#### Audit Item 2.1: False-Positive Explosion on `.env.example`
- **Current Capability:** Treats every single `KEY=VALUE` line in `.env.example` as a mandatory environment variable required in the host machine shell.
- **Actual Limitation:** On `heym`, CONCORD produced 50+ HIGH severity failure predictions for optional tuneables, OTEL flags, and variables with fallback defaults.
- **Why It Matters:** Destroys trust. Developers ignore tools that scream 50 false alarms on a working repository.
- **Proposed Engineering Change:** 
  1. Distinguish environment variable states: `Declared`, `Required`, `OptionalWithDefault`, `PresentInLocalEnv`, `PresentInHostEnv`, `Missing`, `Conflicting`.
  2. Read `.env` and `.env.local` in project root (marked as local presence, not host presence).
  3. Classify variables with non-empty example values as `OptionalWithDefault` unless explicitly marked required (e.g. comment `# required`, or empty string value like `SECRET_KEY=`).

---

### 3. Runtime Constraint Extraction & Conflicts (`concord-constraints`)

#### Audit Item 3.1: Silent Overwriting and Lack of Conflict Detection
- **Current Capability:** Evaluates runtime constraints in isolation.
- **Actual Limitation:** If `.nvmrc` specifies `20` and `package.json` specifies `>=22`, CONCORD does not diagnose the contradiction; it simply checks one or combines them naively.
- **Why It Matters:** Contradictory version specifications across team configuration files (`.nvmrc`, `package.json`, `.tool-versions`, `mise.toml`) cause mysterious CI vs local build failures.
- **Proposed Engineering Change:** Implement a conflict detector in the constraint engine that evaluates cross-source compatibility. If `sources_conflict(sources)`, emit a first-class `ConfigurationConflict` prediction and diagnosis.

---

### 4. Dynamic Probing & Machine Scanner (`concord-scanner`)

#### Audit Item 4.1: Lack of Active Network and Service Probes
- **Current Capability:** Parses Linux `/proc/net/tcp` passively and checks for `/var/run/docker.sock`.
- **Actual Limitation:** Does not verify actual TCP socket connectivity to declared ports or services (e.g. connecting to `127.0.0.1:5432` with a 200ms timeout).
- **Why It Matters:** A port might be in `TCP_LISTEN` state by a zombie process or bound only to a specific interface. Dynamic socket connect probe provides `CONFIRMED` empirical evidence: `connection refused` or `connected`.
- **Proposed Engineering Change:** Add active, non-destructive dynamic socket probes with short timeouts (e.g. `probe_tcp_connect("127.0.0.1", port, timeout)`), clearly marked as `EvidenceSource::DynamicProbe`.

---

### 5. Environment Graph & Causal Diagnosis (`concord-graph` & `concord-diagnosis`)

#### Audit Item 5.1: Synthetic Causal Chains vs Graph Path Traversal
- **Current Capability:** `diagnose_all` uses a hardcoded `match` statement on constraint types to emit pre-canned formatted strings.
- **Actual Limitation:** The environment graph (`petgraph`) is built but barely used for diagnosis. It does not traverse edges: `Component -> Service -> Database -> Port -> Machine Capability`.
- **Why It Matters:** The core value proposition of CONCORD is root-cause causal explanation (e.g. "Backend fails because DATABASE_URL targets localhost:5432, but port 5432 is occupied by an unrelated redis process").
- **Proposed Engineering Change:** 
  1. Build multi-tiered graph nodes: `Component`, `Service`, `Runtime`, `Port`, `EnvVar`, `Constraint`, `MachineCapability`, `Evidence`.
  2. Implement real graph traversal (`find_shortest_path`, `find_causal_ancestors`) to trace from an affected component through intermediate dependencies down to the first violated invariant.

---

### 6. Public JSON Interface & CLI Formatting (`concord-cli`)

#### Audit Item 6.1: Undocumented and Incomplete JSON Contract
- **Current Capability:** Dumps internal structs via `serde_json::to_string_pretty`.
- **Actual Limitation:** No formal schema specification, no versioning of the JSON contract, and graph structure is omitted.
- **Why It Matters:** Other tools, CI scripts, and agentic workflows cannot reliably consume CONCORD output without a stable, documented public schema.
- **Proposed Engineering Change:** Create `docs/JSON_SCHEMA.md`, define a versioned `v1` public JSON API format, and add comprehensive schema roundtrip integration tests.

---

## Action Plan & Phased Implementation

1. **Step 1: Deep Project Analyzer** — Multi-component scanning, Rust & Go support, rich Docker/Compose models, framework default port inference.
2. **Step 2: Environment Variable Refactoring** — Categorization (`Required`, `OptionalWithDefault`, `PresentLocal`, etc.), eliminate false alarms.
3. **Step 3: Multi-Source Runtime Conflict Engine** — Detect contradictory version specifications across `.nvmrc`, `package.json`, `.tool-versions`, etc.
4. **Step 4: Controlled Dynamic Probing** — Active TCP connect probing with timeouts for predicted ports and services.
5. **Step 5: Graph-Based Causal Diagnosis** — Real graph traversal linking Component -> Dependency -> Violated Invariant.
6. **Step 6: CLI & Verification Pipeline Polish** — Graph-driven `concord explain` and structured `concord verify`.
7. **Step 7: JSON API Contract & Documentation** — `docs/JSON_SCHEMA.md` and integration tests.
8. **Step 8: Benchmarks & Real Repo Evaluation** — `benchmarks/` corpus and `docs/BENCHMARKS.md`.

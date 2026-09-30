# CONCORD

> **Development-environment resolution engine that predicts failures before they happen, determines root causes with verifiable evidence, and proves environment invariants.**

Modern development setups fail because code does not run in isolation—it depends on runtimes, system libraries, listening ports, background services, environment variables, and OS capabilities.

Conventional "doctor" commands merely test if a command exists in PATH.

**CONCORD models your project and machine as a constraint graph to answer:**
> *Why will this project fail on this machine, what chain of dependencies causes the failure, and can we prove it before running?*

**Zero AI required.** CONCORD is 100% deterministic systems software written in Rust.

---

## Quick Install (Linux x86_64)

Install the latest release with one command (no Rust, Cargo, Node, or Docker required):

```bash
curl -fsSL https://raw.githubusercontent.com/axonel/concord/main/install.sh | sh
```

The installer verifies SHA256 checksums automatically and places `concord` in `~/.local/bin`.

To install a specific version or custom directory:
```bash
curl -fsSL https://raw.githubusercontent.com/axonel/concord/main/install.sh | CONCORD_VERSION=v0.2.1 sh
```

---

## 30-Second Tour

### 1. Run in your project root

```bash
concord .
```

#### If your environment is compatible:
```text
CONCORD — Development Environment Engine
──────────────────────────────────────────
Project:     .
Name:        my-backend
Languages:   python

Environment: COMPATIBLE

 ✓ Project requirements discovered
 ✓ Runtime versions compatible
 ✓ Required services available
 ✓ Declared ports available
 ✓ Configuration consistent

No known blockers.
```

#### If your environment has contradictions:
```text
CONCORD — Development Environment Engine
──────────────────────────────────────────
Project:     tests/fixtures/broken-python-version
Name:        broken-python-version
Languages:   python

2 problems detected (2 predicted failures)

  HIGH       python runtime incompatibility predicted
             Project expects python >=3.99.0, but Runtime 'python' version 3.12.4 does not satisfy requirement >=3.99.0. Application startup or build is predicted to fail.
  HIGH       Required service 'postgresql' failure predicted
             Project depends on service 'postgresql', but it is not running or not installed. Connections will be refused.

Next steps:
  concord explain     Inspect root causes and causal dependency chains
  concord verify      Run read-only environment verification checks
```

---

### 2. Inspect the causal root causes

```bash
concord explain
```

Traces the dependency graph back to the earliest violated invariant with exact machine provenance:

```text
CONCORD — Development Environment Engine
──────────────────────────────────────────
Root-Cause Diagnosis & Causal Chains

1. python runtime incompatibility predicted
   Root Cause:          python.version >= >=3.99.0
   Violated Constraint: Runtime 'python' must satisfy >=3.99.0
   Confidence:          HIGH
   Affected Components: python, build, startup

   Causal Chain:
     ├─► Host machine state: Runtime 'python' is installed at /usr/bin/python3 (version 3.12.4)
     ├─► Project specification: requires python >=3.99.0
     ├─► Violated invariant: python.version satisfies >=3.99.0
     └─► Downstream impact: python toolchain cannot initialize; build and runtime will fail

2. Required service 'postgresql' failure predicted
   Root Cause:          service.postgresql.running
   Violated Constraint: Service 'postgresql' must be running
   Confidence:          HIGH
   Affected Components: postgresql, database/backend

   Causal Chain:
     ├─► Host service state: Service 'postgresql' status is Stopped
     ├─► Project specification: depends on active service postgresql
     ├─► Violated invariant: service.postgresql.status == RUNNING
     └─► Downstream impact: application connections to postgresql will be refused
```

---

### 3. Read-only verification

Verify all requirements, ports, and services non-destructively:

```bash
concord verify
```

```text
CONCORD — Development Environment Engine
──────────────────────────────────────────
Read-Only Environment Verification

  ✓ [PASSED] project_discovery         Discovered 2 requirements and 0 declared ports across ["python"]
  ✗ [FAILED] runtime:python            Runtime 'python' version 3.12.4 does not satisfy requirement >=3.99.0
  ✗ [FAILED] service:postgresql        Service 'postgresql' is present but currently stopped/inactive

Summary: 1/3 checks passed (2 failed)
Environment verification FAILED.
```

---

### 4. Machine-readable JSON output

Every command supports `--json` for CI/CD pipelines, pre-commit hooks, and developer tooling:

```bash
concord . --json
```

```json
{
  "project": {
    "name": "broken-python-version",
    "root_path": "/workspace/broken-python-version",
    "languages": ["python"],
    "package_managers": [],
    "requirements": [...],
    "declared_ports": [],
    "env_vars": [],
    "docker_used": false,
    "evidence": [...]
  },
  "machine": {
    "os": "Ubuntu 24.04 LTS",
    "arch": "x86_64",
    "cpu_count": 8,
    "runtimes": [...],
    "services": [...],
    "listening_ports": [...]
  },
  "evaluated_constraints": [...],
  "predictions": [
    {
      "title": "python runtime incompatibility predicted",
      "category": "runtime_incompatibility",
      "summary": "Project expects python >=3.99.0, but Runtime 'python' version 3.12.4 does not satisfy requirement >=3.99.0.",
      "confidence": "HIGH",
      "affected_components": ["python", "build", "startup"]
    }
  ],
  "diagnoses": [...],
  "verification": {
    "success": false,
    "checks": [...],
    "total_checks": 3,
    "passed_checks": 1,
    "failed_checks": 2
  }
}
```

---

## Supported Ecosystems (v0.2.1)

| Category | Supported Technologies |
| :--- | :--- |
| **Host OS** | Linux (Ubuntu, Debian, Fedora, Arch, Alpine, etc.) on `x86_64` |
| **Languages & Runtimes** | Node.js, Bun, Python, Rust, Go, Java |
| **Package Managers** | npm, pnpm, yarn, bun, uv, poetry, pipenv, cargo |
| **Containers & Services** | Docker (daemon socket, live containers `docker ps -a`, compose projects), PostgreSQL, Redis |
| **Project Signals** | `package.json`, `bun.lock`, `pyproject.toml`, `requirements.txt`, `uv.lock`, `Dockerfile`, `docker-compose.yml`, `compose.yaml`, `.nvmrc`, `.python-version`, `.tool-versions`, `mise.toml`, `Cargo.toml`, `go.mod`, `.env.example` |

---

## Manual Download & Verification

If you prefer to download release binaries directly from GitHub Releases:

1. Download the archive and SHA256 checksum:
   ```bash
   curl -LO https://github.com/axonel/concord/releases/download/v0.2.1/concord-v0.2.1-linux-x86_64.tar.gz
   curl -LO https://github.com/axonel/concord/releases/download/v0.2.1/concord-v0.2.1-linux-x86_64.tar.gz.sha256
   ```

2. Verify the checksum:
   ```bash
   sha256sum -c concord-v0.2.1-linux-x86_64.tar.gz.sha256
   ```

3. Extract and move to your PATH:
   ```bash
   tar -xzf concord-v0.2.1-linux-x86_64.tar.gz
   mkdir -p ~/.local/bin
   mv concord-v0.2.1-linux-x86_64/concord ~/.local/bin/
   ```

---

## Building From Source

```bash
git clone https://github.com/axonel/concord.git
cd concord
cargo build --release
./target/release/concord --version
```

---

## Architecture & Design

See [ARCHITECTURE.md](ARCHITECTURE.md) for details on:
- The 8-stage deterministic resolution pipeline
- Environment IR and Evidence provenance model
- Data-driven constraint evaluation
- Graph traversal and causal chain tracing
- Planned roadmap (Minimal Repair Planner, Counterfactual Simulation, Transactional Execution)

---

## License

[Apache-2.0](LICENSE) © [Axonel](https://github.com/axonel)

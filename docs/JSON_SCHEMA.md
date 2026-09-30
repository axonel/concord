# CONCORD Public JSON Interface Specification

CONCORD provides a stable, machine-readable JSON contract designed for CI/CD pipelines, agent integrations, developer tooling, and IDE extensions.

All commands support `--json` to output structured data instead of terminal formatting.

---

## 1. Top-Level Execution: `concord [PATH] --json`

Executing `concord . --json` returns a complete `ConcordReport` containing the project manifest, host machine capabilities, evaluated constraints, failure predictions, root-cause diagnoses, and verification report.

### Schema: `ConcordReport`

```json
{
  "project": {
    "name": "string",
    "root_path": "string (path)",
    "languages": ["string"],
    "package_managers": ["string"],
    "requirements": [
      {
        "name": "string",
        "kind": {
          "type": "runtime | package_manager | port | service | os | arch | memory | env_var | conflict",
          "...": "kind-specific fields"
        },
        "evidence": {
          "source": { "type": "string", "...": "source fields" },
          "confidence": "CONFIRMED | HIGH | MEDIUM | LOW | UNKNOWN",
          "description": "string"
        }
      }
    ],
    "declared_ports": ["number (u16)"],
    "env_vars": ["string"],
    "env_var_specs": [
      {
        "key": "string",
        "category": "required | optional_with_default | configured_local",
        "default_value": "string | null",
        "source_file": "string (path)"
      }
    ],
    "components": [
      {
        "name": "string",
        "path": "string (path)",
        "languages": ["string"],
        "package_managers": ["string"],
        "requirements": ["ProjectRequirement"],
        "declared_ports": ["number (u16)"],
        "env_vars": ["string"]
      }
    ],
    "docker_used": "boolean",
    "evidence": ["Evidence"]
  },
  "machine": {
    "os": "string",
    "os_family": "string",
    "arch": "string",
    "cpu_count": "number",
    "total_memory_bytes": "number",
    "available_memory_bytes": "number",
    "runtimes": [
      {
        "name": "string",
        "version": "string",
        "executable_path": "string (path)",
        "evidence": "Evidence"
      }
    ],
    "services": [
      {
        "name": "string",
        "status": "running | stopped | not_installed | unknown",
        "version": "string | null",
        "evidence": "Evidence"
      }
    ],
    "listening_ports": [
      {
        "port": "number (u16)",
        "state": {
          "type": "occupied | free",
          "pid": "number | null",
          "process_name": "string | null"
        },
        "evidence": "Evidence"
      }
    ],
    "env_vars": { "KEY": "VALUE" },
    "path_entries": ["string (path)"],
    "evidence": ["Evidence"]
  },
  "evaluated_constraints": [
    {
      "constraint": {
        "type": "runtime_version | port_available | service_running | memory_min | env_var_set | os_match | arch_match | conflict_detected",
        "...": "constraint fields"
      },
      "status": {
        "type": "satisfied | violated | skipped | unknown",
        "reason": "string (if violated)",
        "root_cause_hint": "string (if violated)"
      },
      "project_evidence": "Evidence | null",
      "machine_evidence": "Evidence | null"
    }
  ],
  "predictions": [
    {
      "title": "string",
      "category": "runtime_incompatibility | port_collision | missing_service | insufficient_resources | configuration_missing | configuration_conflict | os_arch_mismatch",
      "summary": "string",
      "confidence": "CONFIRMED | HIGH | MEDIUM | LOW | UNKNOWN",
      "constraint": "Constraint",
      "affected_components": ["string"],
      "project_evidence": "Evidence | null",
      "machine_evidence": "Evidence | null"
    }
  ],
  "diagnoses": [
    {
      "problem": "string",
      "root_cause": "string",
      "causal_chain": ["string"],
      "violated_constraint": "string",
      "confidence": "CONFIRMED | HIGH | MEDIUM | LOW | UNKNOWN",
      "affected_components": ["string"],
      "project_evidence": "Evidence | null",
      "machine_evidence": "Evidence | null"
    }
  ],
  "verification": {
    "success": "boolean",
    "checks": [
      {
        "name": "string",
        "category": "string",
        "passed": "boolean",
        "message": "string",
        "evidence": "Evidence | null"
      }
    ],
    "total_checks": "number",
    "passed_checks": "number",
    "failed_checks": "number"
  }
}
```

---

## 2. Subcommands with `--json`

### `concord scan [PATH] --json`
Returns `{ "project": ProjectManifest, "machine": MachineCapability }`.

### `concord predict [PATH] --json`
Returns `Vec<Prediction>`.

### `concord explain [PATH] [--target <name>] --json`
Returns `Vec<Diagnosis>`, filtered by optional `--target`.

### `concord verify [PATH] --json`
Returns `VerificationReport`.

---

## 3. Exit Codes

- `0`: All checks passed / no predicted failures.
- `1`: Environment problems or predicted failures detected.
- `2`: Fatal error (e.g. invalid target path, unreadable directory).

# Contributing to CONCORD

CONCORD is deterministic Rust software for inspecting development environments, evaluating project requirements, and explaining predicted failures. Contributions should preserve reproducibility, evidence provenance, and read-only verification.

## Before opening an issue

- Search existing issues and pull requests for duplicates.
- Include the CONCORD version, Linux distribution, architecture, and relevant runtime/tool versions.
- For a detection or prediction bug, provide a minimal project fixture and the expected versus actual result. Remove secrets and personal paths.
- Distinguish observed behavior from a proposed design; avoid sharing `.env` values or private repository contents.

## Development requirements

- Rust stable toolchain (the workspace declares Rust edition 2021).
- Git.
- Linux x86_64 for exercising the currently supported host scanner and packaged release path.

## Repository layout

```text
crates/
  core/         Environment IR, evidence, architecture and version models
  scanner/      Host machine observations
  project/      Repository and build-system discovery
  constraints/  Declarative constraint evaluation
  graph/        Dependency and causality graph
  predictor/    Deterministic failure predictions
  diagnosis/    Root-cause explanations
  verifier/     Read-only environment verification
  cli/          CLI, orchestration, output and integration tests
tests/          Cross-crate tests and minimal project fixtures
docs/           Design notes, model/schema references and audits
.github/        CI and release workflows
```

## Build and validate

Run commands from the repository root:

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release
```

For changes to packaging or installation, also run:

```bash
bash scripts/test-install.sh
```

The clean-room installer test downloads the published release asset, so it requires network access and an available release matching the installer version. Report an unavailable release asset as an environmental blocker; do not silently skip or weaken the test.

## Engineering principles

1. **Evidence first.** Predictions and diagnoses should retain the repository and machine evidence that supports them. Do not convert missing observations into confirmed facts.
2. **Deterministic behavior.** Avoid network calls, model inference, or nondeterministic heuristics in the analysis pipeline.
3. **Read-only verification.** Scanning and verification must not install packages, start services, mutate project files, or change host configuration.
4. **Explicit uncertainty.** Unknown runtime versions, unsupported signals, and ambiguous dependency evidence must remain unknown rather than being reported as certain failures or successes.
5. **Minimal fixtures.** Add focused fixtures and regression tests for new project signals, constraints, and causal paths.
6. **Safe host inspection.** Do not expose environment-variable values, credentials, or unrelated process data in human or JSON output.

## Pull request checklist

- [ ] Explain the user-visible problem and the behavior change.
- [ ] Add or update a minimal fixture for scanner/project-detection changes.
- [ ] Add regression tests for constraint, prediction, diagnosis, or verification changes.
- [ ] Keep JSON output machine-readable and backward-compatible where practical.
- [ ] Run formatting, tests, and Clippy; include exact results and any environmental blockers.
- [ ] Update README/docs when supported behavior, platform scope, or CLI output changes.
- [ ] Do not include secrets, machine-specific paths, generated binaries, or unrelated formatting changes.

Use a focused branch and a conventional commit subject such as `fix(scanner): preserve unknown runtime evidence`. Pull requests should describe the test coverage and any limitations honestly.

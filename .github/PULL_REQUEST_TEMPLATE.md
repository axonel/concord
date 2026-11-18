## Summary

<!-- What changes, and why? -->

## Behavior and evidence

<!-- Describe the previous behavior, the new behavior, and any relevant evidence/provenance changes. -->

## Validation

- [ ] `cargo fmt --check`
- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo build --release` (when relevant)
- [ ] Installer/package smoke test (when relevant)

**Results:**

<!-- Include exact results. Call out unavailable external release assets or other blockers; do not claim unrun checks passed. -->

## Safety and compatibility

- [ ] No secrets or sensitive host details are included.
- [ ] Scan/verify remain read-only.
- [ ] JSON/output and documentation are updated when behavior changes.

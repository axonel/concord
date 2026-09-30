use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use toml::Value;
use concord_core::evidence::Evidence;
use concord_core::ir::{ProjectRequirement, RequirementKind};
use concord_core::version::{compare_version_components, parse_version_components};
use concord_core::Confidence;
use concord_core::VersionConstraint;

pub struct RustDiscovery {
    pub is_rust: bool,
    pub requirements: Vec<ProjectRequirement>,
    pub package_managers: Vec<String>,
    pub evidence: Vec<Evidence>,
}

fn get_manifest_string_or_workspace(toml: &Value, section: &str, key: &str) -> Option<String> {
    if let Some(sec_val) = toml.get(section).and_then(|s| s.get(key)) {
        if let Some(s) = sec_val.as_str() {
            return Some(s.to_string());
        }
        if sec_val.get("workspace").and_then(|w| w.as_bool()) == Some(true) {
            if let Some(ws_val) = toml
                .get("workspace")
                .and_then(|w| w.get("package"))
                .and_then(|p| p.get(key))
                .and_then(|v| v.as_str())
            {
                return Some(ws_val.to_string());
            }
        }
    }
    // Also check workspace.package directly if not found in package section
    toml.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get(key))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub fn edition_to_minimum_rustc(edition: &str) -> Option<&'static str> {
    match edition.trim() {
        "2024" => Some("1.85.0"),
        "2021" => Some("1.56.0"),
        "2018" => Some("1.31.0"),
        "2015" => Some("1.0.0"),
        _ => None,
    }
}

/// Analyze Rust projects (Cargo.toml, rust-toolchain.toml, rust-toolchain).
pub fn analyze_rust(root: &Path) -> RustDiscovery {
    let mut is_rust = false;
    let mut requirements = Vec::new();
    let mut package_managers = Vec::new();
    let mut evidence = Vec::new();

    let cargo_toml_path = root.join("Cargo.toml");
    if cargo_toml_path.exists() {
        is_rust = true;
        package_managers.push("cargo".to_string());

        let ev = Evidence::from_repo_file(
            PathBuf::from("Cargo.toml"),
            None,
            "Rust project detected via Cargo.toml",
        );
        evidence.push(ev);

        match fs::read_to_string(&cargo_toml_path) {
            Ok(content) => match content.parse::<Value>() {
                Ok(toml) => {
                    // 1. Resolve rust-version (MSRV) and edition
                    let rust_ver =
                        get_manifest_string_or_workspace(&toml, "package", "rust-version");
                    let edition_str = get_manifest_string_or_workspace(&toml, "package", "edition");
                    let edition_min_ver = edition_str.as_deref().and_then(edition_to_minimum_rustc);

                    let effective_req = match (rust_ver.as_deref(), edition_min_ver) {
                        (Some(rv), Some(ev)) => {
                            let rv_comp = parse_version_components(rv);
                            let ev_comp = parse_version_components(ev);
                            if compare_version_components(&rv_comp, &ev_comp) == Ordering::Less {
                                Some((
                                    ev.to_string(),
                                    format!(
                                        "Rust edition {} requires rustc >= {} (higher than declared rust-version {})",
                                        edition_str.as_deref().unwrap_or_default(),
                                        ev,
                                        rv
                                    ),
                                ))
                            } else {
                                Some((
                                    rv.to_string(),
                                    format!(
                                        "Rust MSRV declared in rust-version: {} (edition {})",
                                        rv,
                                        edition_str.as_deref().unwrap_or_default()
                                    ),
                                ))
                            }
                        }
                        (Some(rv), None) => Some((
                            rv.to_string(),
                            format!("Rust MSRV declared in rust-version: {}", rv),
                        )),
                        (None, Some(ev)) => Some((
                            ev.to_string(),
                            format!(
                                "Rust edition {} requires compiler version >= {}",
                                edition_str.as_deref().unwrap_or_default(),
                                ev
                            ),
                        )),
                        (None, None) => None,
                    };

                    if let Some((min_ver, detail)) = effective_req {
                        let ev =
                            Evidence::from_repo_file(PathBuf::from("Cargo.toml"), None, detail);
                        requirements.push(ProjectRequirement::new(
                            "rust",
                            RequirementKind::Runtime {
                                name: "rust".to_string(),
                                constraint: VersionConstraint::GreaterEqual(min_ver),
                            },
                            ev.clone(),
                        ));
                        evidence.push(ev);
                    } else {
                        // General Rust runtime requirement
                        let ev = Evidence::new(
                            concord_core::evidence::EvidenceSource::RepositoryFile {
                                path: PathBuf::from("Cargo.toml"),
                                line: None,
                                detail: Some("Cargo project configuration present".to_string()),
                            },
                            Confidence::High,
                            "Rust toolchain required by Cargo project",
                        );
                        requirements.push(ProjectRequirement::new(
                            "rust",
                            RequirementKind::Runtime {
                                name: "rust".to_string(),
                                constraint: VersionConstraint::Any,
                            },
                            ev.clone(),
                        ));
                        evidence.push(ev);
                    }

                    // 2. Check for database client dependencies (sqlx, diesel, tokio-postgres, postgres)
                    let check_db = |name: &str| -> bool {
                        let in_deps = toml.get("dependencies").and_then(|d| d.get(name)).is_some();
                        let in_ws_deps = toml
                            .get("workspace")
                            .and_then(|w| w.get("dependencies"))
                            .and_then(|d| d.get(name))
                            .is_some();
                        in_deps || in_ws_deps
                    };

                    if check_db("sqlx")
                        || check_db("diesel")
                        || check_db("postgres")
                        || check_db("tokio-postgres")
                    {
                        let ev = Evidence::from_repo_file(
                            PathBuf::from("Cargo.toml"),
                            None,
                            "PostgreSQL client crate detected in Cargo dependencies",
                        );
                        requirements.push(ProjectRequirement::new(
                            "postgresql",
                            RequirementKind::Service {
                                name: "postgresql".to_string(),
                                min_version: None,
                            },
                            ev.clone(),
                        ));
                        evidence.push(ev);
                    }
                }
                Err(e) => {
                    evidence.push(Evidence::new(
                        concord_core::evidence::EvidenceSource::RepositoryFile {
                            path: PathBuf::from("Cargo.toml"),
                            line: None,
                            detail: Some(e.to_string()),
                        },
                        Confidence::Confirmed,
                        format!("Syntax error in Cargo.toml: {}", e),
                    ));
                }
            },
            Err(e) => {
                evidence.push(Evidence::new(
                    concord_core::evidence::EvidenceSource::RepositoryFile {
                        path: PathBuf::from("Cargo.toml"),
                        line: None,
                        detail: Some(e.to_string()),
                    },
                    Confidence::Confirmed,
                    format!("Failed to read Cargo.toml: {}", e),
                ));
            }
        }
    }

    // 2. Check rust-toolchain.toml or rust-toolchain
    let toolchain_toml = root.join("rust-toolchain.toml");
    let toolchain_plain = root.join("rust-toolchain");

    if toolchain_toml.exists() {
        is_rust = true;
        if let Ok(content) = fs::read_to_string(&toolchain_toml) {
            if let Ok(toml) = content.parse::<Value>() {
                if let Some(channel) = toml
                    .get("toolchain")
                    .and_then(|t| t.get("channel"))
                    .and_then(|c| c.as_str())
                {
                    let ev = Evidence::from_repo_file(
                        PathBuf::from("rust-toolchain.toml"),
                        None,
                        format!("Rust toolchain channel specified: {}", channel),
                    );
                    requirements.push(ProjectRequirement::new(
                        "rust",
                        RequirementKind::Runtime {
                            name: "rust".to_string(),
                            constraint: VersionConstraint::parse(channel),
                        },
                        ev.clone(),
                    ));
                    evidence.push(ev);
                }
            }
        }
    } else if toolchain_plain.exists() {
        is_rust = true;
        if let Ok(content) = fs::read_to_string(&toolchain_plain) {
            let channel = content.trim();
            if !channel.is_empty() {
                let ev = Evidence::from_repo_file(
                    PathBuf::from("rust-toolchain"),
                    None,
                    format!("Rust toolchain channel specified: {}", channel),
                );
                requirements.push(ProjectRequirement::new(
                    "rust",
                    RequirementKind::Runtime {
                        name: "rust".to_string(),
                        constraint: VersionConstraint::parse(channel),
                    },
                    ev.clone(),
                ));
                evidence.push(ev);
            }
        }
    }

    RustDiscovery {
        is_rust,
        requirements,
        package_managers,
        evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_rust_edition_2024_infers_1_85() {
        let dir = tempdir().unwrap();
        let cargo_toml = r#"[package]
name = "test-pkg"
version = "0.1.0"
edition = "2024"
"#;
        fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();

        let disc = analyze_rust(dir.path());
        assert!(disc.is_rust);
        let rust_req = disc.requirements.iter().find(|r| r.name == "rust").unwrap();
        if let RequirementKind::Runtime { constraint, .. } = &rust_req.kind {
            assert_eq!(
                constraint,
                &VersionConstraint::GreaterEqual("1.85.0".to_string())
            );
        } else {
            panic!("Expected runtime requirement");
        }
    }

    #[test]
    fn test_rust_edition_workspace_inheritance() {
        let dir = tempdir().unwrap();
        let cargo_toml = r#"[package]
name = "test-pkg"
version = "0.1.0"
edition.workspace = true

[workspace.package]
edition = "2024"
"#;
        fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();

        let disc = analyze_rust(dir.path());
        assert!(disc.is_rust);
        let rust_req = disc.requirements.iter().find(|r| r.name == "rust").unwrap();
        if let RequirementKind::Runtime { constraint, .. } = &rust_req.kind {
            assert_eq!(
                constraint,
                &VersionConstraint::GreaterEqual("1.85.0".to_string())
            );
        } else {
            panic!("Expected runtime requirement");
        }
    }

    #[test]
    fn test_rust_msrv_higher_than_edition() {
        let dir = tempdir().unwrap();
        let cargo_toml = r#"[package]
name = "test-pkg"
version = "0.1.0"
edition = "2021"
rust-version = "1.80.0"
"#;
        fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();

        let disc = analyze_rust(dir.path());
        let rust_req = disc.requirements.iter().find(|r| r.name == "rust").unwrap();
        if let RequirementKind::Runtime { constraint, .. } = &rust_req.kind {
            assert_eq!(
                constraint,
                &VersionConstraint::GreaterEqual("1.80.0".to_string())
            );
        } else {
            panic!("Expected runtime requirement");
        }
    }

    #[test]
    fn test_rust_edition_higher_than_msrv() {
        let dir = tempdir().unwrap();
        let cargo_toml = r#"[package]
name = "test-pkg"
version = "0.1.0"
edition = "2024"
rust-version = "1.75.0"
"#;
        fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();

        let disc = analyze_rust(dir.path());
        let rust_req = disc.requirements.iter().find(|r| r.name == "rust").unwrap();
        if let RequirementKind::Runtime { constraint, .. } = &rust_req.kind {
            assert_eq!(
                constraint,
                &VersionConstraint::GreaterEqual("1.85.0".to_string())
            );
        } else {
            panic!("Expected runtime requirement");
        }
    }

    #[test]
    fn test_rust_pure_workspace_package() {
        let dir = tempdir().unwrap();
        let cargo_toml = r#"[workspace]
members = ["subcrate"]

[workspace.package]
edition = "2024"
"#;
        fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();

        let disc = analyze_rust(dir.path());
        assert!(disc.is_rust);
        let rust_req = disc.requirements.iter().find(|r| r.name == "rust").unwrap();
        if let RequirementKind::Runtime { constraint, .. } = &rust_req.kind {
            assert_eq!(
                constraint,
                &VersionConstraint::GreaterEqual("1.85.0".to_string())
            );
        } else {
            panic!("Expected runtime requirement");
        }
    }

    #[test]
    fn test_rust_no_version_falls_back_to_any() {
        let dir = tempdir().unwrap();
        let cargo_toml = r#"[package]
name = "test-pkg"
version = "0.1.0"
"#;
        fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();

        let disc = analyze_rust(dir.path());
        assert!(disc.is_rust);
        let rust_req = disc.requirements.iter().find(|r| r.name == "rust").unwrap();
        if let RequirementKind::Runtime { constraint, .. } = &rust_req.kind {
            assert_eq!(constraint, &VersionConstraint::Any);
        } else {
            panic!("Expected runtime requirement");
        }
    }
}

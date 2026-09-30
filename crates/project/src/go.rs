use std::fs;
use std::path::{Path, PathBuf};
use concord_core::evidence::Evidence;
use concord_core::ir::{ProjectRequirement, RequirementKind};
use concord_core::Confidence;
use concord_core::VersionConstraint;

pub struct GoDiscovery {
    pub is_go: bool,
    pub requirements: Vec<ProjectRequirement>,
    pub package_managers: Vec<String>,
    pub evidence: Vec<Evidence>,
}

/// Analyze Go projects (go.mod, go.sum).
pub fn analyze_go(root: &Path) -> GoDiscovery {
    let mut is_go = false;
    let mut requirements = Vec::new();
    let mut package_managers = Vec::new();
    let mut evidence = Vec::new();

    let gomod_path = root.join("go.mod");
    if gomod_path.exists() {
        is_go = true;
        package_managers.push("go".to_string());

        let ev = Evidence::from_repo_file(
            PathBuf::from("go.mod"),
            None,
            "Go module detected via go.mod",
        );
        evidence.push(ev);

        match fs::read_to_string(&gomod_path) {
            Ok(content) => {
                let mut found_ver = false;
                for (idx, line) in content.lines().enumerate() {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix("go ") {
                        let ver = rest.trim();
                        if !ver.is_empty() {
                            found_ver = true;
                            let ev = Evidence::from_repo_file(
                                PathBuf::from("go.mod"),
                                Some(idx + 1),
                                format!("Go language version declared: {}", ver),
                            );
                            requirements.push(ProjectRequirement::new(
                                "go",
                                RequirementKind::Runtime {
                                    name: "go".to_string(),
                                    constraint: VersionConstraint::GreaterEqual(ver.to_string()),
                                },
                                ev.clone(),
                            ));
                            evidence.push(ev);
                        }
                    }

                    // Check for PostgreSQL drivers
                    if trimmed.contains("github.com/lib/pq")
                        || trimmed.contains("github.com/jackc/pgx")
                        || trimmed.contains("gorm.io/driver/postgres")
                    {
                        let ev = Evidence::from_repo_file(
                            PathBuf::from("go.mod"),
                            Some(idx + 1),
                            format!("PostgreSQL Go driver detected: {}", trimmed),
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

                if !found_ver {
                    let ev = Evidence::new(
                        concord_core::evidence::EvidenceSource::RepositoryFile {
                            path: PathBuf::from("go.mod"),
                            line: None,
                            detail: Some(
                                "Go module present without explicit minimum version".to_string(),
                            ),
                        },
                        Confidence::High,
                        "Go toolchain required by project",
                    );
                    requirements.push(ProjectRequirement::new(
                        "go",
                        RequirementKind::Runtime {
                            name: "go".to_string(),
                            constraint: VersionConstraint::Any,
                        },
                        ev.clone(),
                    ));
                    evidence.push(ev);
                }
            }
            Err(e) => {
                evidence.push(Evidence::new(
                    concord_core::evidence::EvidenceSource::RepositoryFile {
                        path: PathBuf::from("go.mod"),
                        line: None,
                        detail: Some(e.to_string()),
                    },
                    Confidence::Confirmed,
                    format!("Failed to read go.mod: {}", e),
                ));
            }
        }
    }

    GoDiscovery {
        is_go,
        requirements,
        package_managers,
        evidence,
    }
}

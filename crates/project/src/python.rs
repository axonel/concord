use std::fs;
use std::path::{Path, PathBuf};
use toml::Value;
use concord_core::evidence::Evidence;
use concord_core::ir::{ProjectRequirement, RequirementKind};
use concord_core::Confidence;
use concord_core::VersionConstraint;

pub struct PythonDiscovery {
    pub is_python: bool,
    pub package_managers: Vec<String>,
    pub requirements: Vec<ProjectRequirement>,
    pub ports: Vec<u16>,
    pub evidence: Vec<Evidence>,
}

pub fn analyze_python(root: &Path) -> PythonDiscovery {
    let mut is_python = false;
    let mut package_managers = Vec::new();
    let mut requirements = Vec::new();
    let mut ports = Vec::new();
    let mut evidence = Vec::new();

    // 1. Lockfiles
    if root.join("uv.lock").exists() {
        is_python = true;
        package_managers.push("uv".to_string());
        evidence.push(Evidence::from_repo_file(
            PathBuf::from("uv.lock"),
            None,
            "uv lockfile detected",
        ));
    }
    if root.join("poetry.lock").exists() {
        is_python = true;
        package_managers.push("poetry".to_string());
        evidence.push(Evidence::from_repo_file(
            PathBuf::from("poetry.lock"),
            None,
            "Poetry lockfile detected",
        ));
    }
    if root.join("Pipfile").exists() || root.join("Pipfile.lock").exists() {
        is_python = true;
        package_managers.push("pipenv".to_string());
        evidence.push(Evidence::from_repo_file(
            PathBuf::from("Pipfile"),
            None,
            "Pipfile detected",
        ));
    }

    // 2. .python-version
    let py_ver_path = root.join(".python-version");
    if py_ver_path.exists() {
        if let Ok(content) = fs::read_to_string(&py_ver_path) {
            let ver = content.trim().to_string();
            if !ver.is_empty() {
                is_python = true;
                let ev = Evidence::from_repo_file(
                    PathBuf::from(".python-version"),
                    Some(1),
                    format!("Python version specified in .python-version: {}", ver),
                );
                requirements.push(ProjectRequirement::new(
                    "python",
                    RequirementKind::Runtime {
                        name: "python".to_string(),
                        constraint: VersionConstraint::parse(&ver),
                    },
                    ev.clone(),
                ));
                evidence.push(ev);
            }
        }
    }

    // 3. pyproject.toml
    let pyproject_path = root.join("pyproject.toml");
    if pyproject_path.exists() {
        is_python = true;
        match fs::read_to_string(&pyproject_path) {
            Ok(content) => match content.parse::<Value>() {
                Ok(toml) => {
                    let mut has_py_req = false;

                    // Check [project] requires-python
                    if let Some(req_py) = toml
                        .get("project")
                        .and_then(|p| p.get("requires-python"))
                        .and_then(|v| v.as_str())
                    {
                        has_py_req = true;
                        let ev = Evidence::from_repo_file(
                            PathBuf::from("pyproject.toml"),
                            None,
                            format!(
                                "Python requirement declared in [project].requires-python: {}",
                                req_py
                            ),
                        );
                        requirements.push(ProjectRequirement::new(
                            "python",
                            RequirementKind::Runtime {
                                name: "python".to_string(),
                                constraint: VersionConstraint::parse(req_py),
                            },
                            ev.clone(),
                        ));
                        evidence.push(ev);
                    }

                    // Check [tool.poetry.dependencies.python]
                    if let Some(poetry_py) = toml
                        .get("tool")
                        .and_then(|t| t.get("poetry"))
                        .and_then(|p| p.get("dependencies"))
                        .and_then(|d| d.get("python"))
                        .and_then(|v| v.as_str())
                    {
                        has_py_req = true;
                        let ev = Evidence::from_repo_file(
                            PathBuf::from("pyproject.toml"),
                            None,
                            format!(
                                "Python requirement declared in Poetry dependencies: {}",
                                poetry_py
                            ),
                        );
                        requirements.push(ProjectRequirement::new(
                            "python",
                            RequirementKind::Runtime {
                                name: "python".to_string(),
                                constraint: VersionConstraint::parse(poetry_py),
                            },
                            ev.clone(),
                        ));
                        evidence.push(ev);
                    }

                    // Baseline Python requirement if no specific version constraint declared
                    if !has_py_req && requirements.iter().all(|r| r.name != "python") {
                        let ev = Evidence::new(
                            concord_core::evidence::EvidenceSource::RepositoryFile {
                                path: PathBuf::from("pyproject.toml"),
                                line: None,
                                detail: Some("pyproject.toml present".to_string()),
                            },
                            Confidence::High,
                            "Python runtime required by pyproject.toml",
                        );
                        requirements.push(ProjectRequirement::new(
                            "python",
                            RequirementKind::Runtime {
                                name: "python".to_string(),
                                constraint: VersionConstraint::Any,
                            },
                            ev.clone(),
                        ));
                        evidence.push(ev);
                    }

                    // Check dependencies
                    let check_dep = |name: &str| -> bool {
                        let in_project = toml
                            .get("project")
                            .and_then(|p| p.get("dependencies"))
                            .and_then(|d| d.as_array())
                            .map(|arr| {
                                arr.iter().any(|v| {
                                    v.as_str().map(|s| s.starts_with(name)).unwrap_or(false)
                                })
                            })
                            .unwrap_or(false);

                        let in_poetry = toml
                            .get("tool")
                            .and_then(|t| t.get("poetry"))
                            .and_then(|p| p.get("dependencies"))
                            .and_then(|d| d.get(name))
                            .is_some();

                        in_project || in_poetry
                    };

                    // PostgreSQL drivers
                    if check_dep("psycopg") || check_dep("psycopg2") || check_dep("asyncpg") {
                        let ev = Evidence::from_repo_file(
                            PathBuf::from("pyproject.toml"),
                            None,
                            "PostgreSQL driver detected in Python dependencies",
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

                    // Web Framework default ports
                    if check_dep("fastapi") || check_dep("uvicorn") {
                        if !ports.contains(&8000) {
                            ports.push(8000);
                            let ev = Evidence::from_repo_file(
                                PathBuf::from("pyproject.toml"),
                                None,
                                "Default port 8000 inferred from FastAPI/Uvicorn dependency",
                            );
                            requirements.push(ProjectRequirement::new(
                                "port:8000",
                                RequirementKind::Port {
                                    port: 8000,
                                    service_hint: Some("fastapi".to_string()),
                                },
                                ev.clone(),
                            ));
                            evidence.push(ev);
                        }
                    } else if check_dep("django") {
                        if !ports.contains(&8000) {
                            ports.push(8000);
                            let ev = Evidence::from_repo_file(
                                PathBuf::from("pyproject.toml"),
                                None,
                                "Default port 8000 inferred from Django dependency",
                            );
                            requirements.push(ProjectRequirement::new(
                                "port:8000",
                                RequirementKind::Port {
                                    port: 8000,
                                    service_hint: Some("django".to_string()),
                                },
                                ev.clone(),
                            ));
                            evidence.push(ev);
                        } else if check_dep("flask") && !ports.contains(&5000) {
                            ports.push(5000);
                            let ev = Evidence::from_repo_file(
                                PathBuf::from("pyproject.toml"),
                                None,
                                "Default port 5000 inferred from Flask dependency",
                            );
                            requirements.push(ProjectRequirement::new(
                                "port:5000",
                                RequirementKind::Port {
                                    port: 5000,
                                    service_hint: Some("flask".to_string()),
                                },
                                ev.clone(),
                            ));
                            evidence.push(ev);
                        }
                    }
                }
                Err(e) => {
                    evidence.push(Evidence::new(
                        concord_core::evidence::EvidenceSource::RepositoryFile {
                            path: PathBuf::from("pyproject.toml"),
                            line: None,
                            detail: Some(e.to_string()),
                        },
                        Confidence::Confirmed,
                        format!("Syntax error in pyproject.toml: {}", e),
                    ));
                }
            },
            Err(e) => {
                evidence.push(Evidence::new(
                    concord_core::evidence::EvidenceSource::RepositoryFile {
                        path: PathBuf::from("pyproject.toml"),
                        line: None,
                        detail: Some(e.to_string()),
                    },
                    Confidence::Confirmed,
                    format!("Failed to read pyproject.toml: {}", e),
                ));
            }
        }
    }

    // 4. requirements.txt
    let req_txt_path = root.join("requirements.txt");
    if req_txt_path.exists() {
        is_python = true;
        if !package_managers.contains(&"pip".to_string()) {
            package_managers.push("pip".to_string());
        }

        if let Ok(content) = fs::read_to_string(&req_txt_path) {
            let mut found_pg = false;
            let mut found_fastapi = false;
            let mut found_flask = false;
            let mut found_django = false;

            for (idx, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("psycopg") || trimmed.starts_with("asyncpg") {
                    if !found_pg {
                        found_pg = true;
                        let ev = Evidence::from_repo_file(
                            PathBuf::from("requirements.txt"),
                            Some(idx + 1),
                            format!(
                                "PostgreSQL driver detected in requirements.txt: {}",
                                trimmed
                            ),
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
                } else if trimmed.starts_with("fastapi") || trimmed.starts_with("uvicorn") {
                    found_fastapi = true;
                } else if trimmed.starts_with("flask") {
                    found_flask = true;
                } else if trimmed.starts_with("django") {
                    found_django = true;
                }
            }

            if found_fastapi && !ports.contains(&8000) {
                ports.push(8000);
            }
            if found_django && !ports.contains(&8000) {
                ports.push(8000);
            }
            if found_flask && !ports.contains(&5000) {
                ports.push(5000);
            }

            if requirements.iter().all(|r| r.name != "python") {
                let ev = Evidence::new(
                    concord_core::evidence::EvidenceSource::RepositoryFile {
                        path: PathBuf::from("requirements.txt"),
                        line: None,
                        detail: Some("requirements.txt present".to_string()),
                    },
                    Confidence::High,
                    "Python runtime required by requirements.txt",
                );
                requirements.push(ProjectRequirement::new(
                    "python",
                    RequirementKind::Runtime {
                        name: "python".to_string(),
                        constraint: VersionConstraint::Any,
                    },
                    ev.clone(),
                ));
                evidence.push(ev);
            }
        }
    }

    PythonDiscovery {
        is_python,
        package_managers,
        requirements,
        ports,
        evidence,
    }
}

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use concord_core::evidence::Evidence;
use concord_core::ir::{ProjectRequirement, RequirementKind};
use concord_core::Confidence;
use concord_core::VersionConstraint;

pub struct NodeDiscovery {
    pub is_node: bool,
    pub is_bun: bool,
    pub package_managers: Vec<String>,
    pub requirements: Vec<ProjectRequirement>,
    pub ports: Vec<u16>,
    pub scripts: Vec<String>,
    pub workspaces: Vec<String>,
    pub evidence: Vec<Evidence>,
}

fn scan_text_for_ports(text: &str, ports: &mut Vec<u16>) {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for i in 0..tokens.len() {
        if tokens[i] == "--port" || tokens[i] == "-p" {
            if let Some(next) = tokens.get(i + 1) {
                if let Ok(p) = next.parse::<u16>() {
                    if !ports.contains(&p) && p > 0 {
                        ports.push(p);
                    }
                }
            }
        } else if let Some(rest) = tokens[i].strip_prefix("PORT=") {
            if let Ok(p) = rest.parse::<u16>() {
                if !ports.contains(&p) && p > 0 {
                    ports.push(p);
                }
            }
        } else if let Some(rest) = tokens[i].strip_prefix("--port=") {
            if let Ok(p) = rest.parse::<u16>() {
                if !ports.contains(&p) && p > 0 {
                    ports.push(p);
                }
            }
        }
    }
}

/// Extract major version number from a version specifier like "^22.16.5" or ">=20.0.0".
fn extract_major_version(ver: &str) -> Option<u32> {
    let clean = ver
        .trim()
        .trim_start_matches(['^', '~', '>', '=', 'v', ' ']);
    let first = clean.split(['.', '-', ' ']).next()?;
    first.parse::<u32>().ok()
}

pub fn analyze_node(root: &Path) -> NodeDiscovery {
    let mut is_node = false;
    let mut is_bun = false;
    let mut package_managers = Vec::new();
    let mut requirements = Vec::new();
    let mut ports = Vec::new();
    let mut scripts = Vec::new();
    let mut workspaces = Vec::new();
    let mut evidence = Vec::new();

    // 1. Check lockfiles
    if root.join("bun.lock").exists() || root.join("bun.lockb").exists() {
        is_bun = true;
        is_node = true;
        package_managers.push("bun".to_string());
        evidence.push(Evidence::from_repo_file(
            PathBuf::from("bun.lock"),
            None,
            "Bun lockfile detected",
        ));
    }
    if root.join("pnpm-lock.yaml").exists() {
        is_node = true;
        package_managers.push("pnpm".to_string());
        evidence.push(Evidence::from_repo_file(
            PathBuf::from("pnpm-lock.yaml"),
            None,
            "pnpm lockfile detected",
        ));
    }
    if root.join("yarn.lock").exists() {
        is_node = true;
        package_managers.push("yarn".to_string());
        evidence.push(Evidence::from_repo_file(
            PathBuf::from("yarn.lock"),
            None,
            "yarn lockfile detected",
        ));
    }
    if root.join("package-lock.json").exists() {
        is_node = true;
        package_managers.push("npm".to_string());
        evidence.push(Evidence::from_repo_file(
            PathBuf::from("package-lock.json"),
            None,
            "npm lockfile detected",
        ));
    }

    // 2. Check .nvmrc and .node-version
    for nvm_file in [".nvmrc", ".node-version"] {
        let nvm_path = root.join(nvm_file);
        if nvm_path.exists() {
            if let Ok(content) = fs::read_to_string(&nvm_path) {
                let ver = content.trim().trim_start_matches('v').to_string();
                if !ver.is_empty() {
                    is_node = true;
                    let ev = Evidence::from_repo_file(
                        PathBuf::from(nvm_file),
                        Some(1),
                        format!("Node version specified in {}: {}", nvm_file, ver),
                    );
                    requirements.push(ProjectRequirement::new(
                        "node",
                        RequirementKind::Runtime {
                            name: "node".to_string(),
                            constraint: VersionConstraint::parse(&ver),
                        },
                        ev.clone(),
                    ));
                    evidence.push(ev);
                }
            }
        }
    }

    // 3. Check package.json
    let pkg_path = root.join("package.json");
    if pkg_path.exists() {
        is_node = true;
        match fs::read_to_string(&pkg_path) {
            Ok(content) => match serde_json::from_str::<Value>(&content) {
                Ok(json) => {
                    let mut has_node_req = false;

                    // engines.node and engines.<pm>
                    if let Some(engines) = json.get("engines") {
                        if let Some(node_engine) = engines.get("node").and_then(|v| v.as_str()) {
                            has_node_req = true;
                            let ev = Evidence::from_repo_file(
                                PathBuf::from("package.json"),
                                None,
                                format!("Node version declared in engines.node: {}", node_engine),
                            );
                            requirements.push(ProjectRequirement::new(
                                "node",
                                RequirementKind::Runtime {
                                    name: "node".to_string(),
                                    constraint: VersionConstraint::parse(node_engine),
                                },
                                ev.clone(),
                            ));
                            evidence.push(ev);
                        }

                        for pm in ["pnpm", "npm", "yarn", "bun"] {
                            if let Some(pm_engine) = engines.get(pm).and_then(|v| v.as_str()) {
                                if !package_managers.contains(&pm.to_string()) {
                                    package_managers.push(pm.to_string());
                                }
                                let ev = Evidence::from_repo_file(
                                    PathBuf::from("package.json"),
                                    None,
                                    format!(
                                        "{} version declared in engines.{}: {}",
                                        pm, pm, pm_engine
                                    ),
                                );
                                requirements.push(ProjectRequirement::new(
                                    pm,
                                    RequirementKind::PackageManager {
                                        name: pm.to_string(),
                                        constraint: Some(VersionConstraint::parse(pm_engine)),
                                    },
                                    ev.clone(),
                                ));
                                evidence.push(ev);
                            }
                        }
                    }

                    // packageManager field (e.g. "pnpm@11.24.0")
                    if let Some(pm) = json.get("packageManager").and_then(|v| v.as_str()) {
                        let parts: Vec<&str> = pm.split('@').collect();
                        let pm_name = parts[0].to_string();
                        let pm_ver = parts.get(1).map(|s| s.to_string());
                        if !package_managers.contains(&pm_name) {
                            package_managers.push(pm_name.clone());
                        }
                        let ev = Evidence::from_repo_file(
                            PathBuf::from("package.json"),
                            None,
                            format!("Package manager specified: {}", pm),
                        );
                        let parsed_pm_constraint = pm_ver.as_deref().map(VersionConstraint::parse);
                        requirements.push(ProjectRequirement::new(
                            pm_name.clone(),
                            RequirementKind::PackageManager {
                                name: pm_name,
                                constraint: parsed_pm_constraint,
                            },
                            ev.clone(),
                        ));
                        evidence.push(ev);
                    }

                    // Workspaces detection
                    if let Some(ws) = json.get("workspaces") {
                        if let Some(arr) = ws.as_array() {
                            for item in arr {
                                if let Some(s) = item.as_str() {
                                    workspaces.push(s.to_string());
                                }
                            }
                        } else if let Some(obj) = ws.as_object() {
                            if let Some(arr) = obj.get("packages").and_then(|p| p.as_array()) {
                                for item in arr {
                                    if let Some(s) = item.as_str() {
                                        workspaces.push(s.to_string());
                                    }
                                }
                            }
                        }
                    }

                    // Scripts & explicit port scanning
                    if let Some(scripts_obj) = json.get("scripts").and_then(|v| v.as_object()) {
                        for (name, script_val) in scripts_obj {
                            scripts.push(name.clone());
                            if let Some(script_str) = script_val.as_str() {
                                scan_text_for_ports(script_str, &mut ports);
                            }
                        }
                    }

                    // Check dependencies and devDependencies
                    let has_dep = |dep_name: &str| -> bool {
                        let in_deps = json
                            .get("dependencies")
                            .and_then(|d| d.get(dep_name))
                            .is_some();
                        let in_dev = json
                            .get("devDependencies")
                            .and_then(|d| d.get(dep_name))
                            .is_some();
                        in_deps || in_dev
                    };

                    // Check @types/node for fallback Node version
                    if !has_node_req {
                        if let Some(types_node) = json
                            .get("devDependencies")
                            .and_then(|d| d.get("@types/node"))
                            .and_then(|v| v.as_str())
                        {
                            if let Some(major) = extract_major_version(types_node) {
                                has_node_req = true;
                                let ev = Evidence::new(
                                    concord_core::evidence::EvidenceSource::RepositoryFile {
                                        path: PathBuf::from("package.json"),
                                        line: None,
                                        detail: Some(format!("@types/node: {}", types_node)),
                                    },
                                    Confidence::High,
                                    format!(
                                        "Node runtime target inferred from @types/node: >={}.0.0",
                                        major
                                    ),
                                );
                                requirements.push(ProjectRequirement::new(
                                    "node",
                                    RequirementKind::Runtime {
                                        name: "node".to_string(),
                                        constraint: VersionConstraint::GreaterEqual(format!(
                                            "{}.0.0",
                                            major
                                        )),
                                    },
                                    ev.clone(),
                                ));
                                evidence.push(ev);
                            }
                        }
                    }

                    // Baseline Node requirement if package.json exists
                    if !has_node_req && requirements.iter().all(|r| r.name != "node") {
                        let ev = Evidence::new(
                            concord_core::evidence::EvidenceSource::RepositoryFile {
                                path: PathBuf::from("package.json"),
                                line: None,
                                detail: Some("package.json present".to_string()),
                            },
                            Confidence::High,
                            "Node.js runtime required by package.json",
                        );
                        requirements.push(ProjectRequirement::new(
                            "node",
                            RequirementKind::Runtime {
                                name: "node".to_string(),
                                constraint: VersionConstraint::Any,
                            },
                            ev.clone(),
                        ));
                        evidence.push(ev);
                    }

                    // Framework default port inference
                    let check_script_contains = |keyword: &str| -> bool {
                        scripts.iter().any(|s| s.contains(keyword))
                            || json
                                .get("scripts")
                                .and_then(|s| s.as_object())
                                .map(|m| {
                                    m.values()
                                        .any(|v| v.as_str().unwrap_or("").contains(keyword))
                                })
                                .unwrap_or(false)
                    };

                    if has_dep("vite") || check_script_contains("vite") {
                        if !ports.contains(&5173) && ports.is_empty() {
                            ports.push(5173);
                            let ev = Evidence::from_repo_file(
                                PathBuf::from("package.json"),
                                None,
                                "Default port 5173 inferred from Vite configuration",
                            );
                            requirements.push(ProjectRequirement::new(
                                "port:5173",
                                RequirementKind::Port {
                                    port: 5173,
                                    service_hint: Some("vite".to_string()),
                                },
                                ev.clone(),
                            ));
                            evidence.push(ev);
                        }
                    } else if has_dep("next") || check_script_contains("next") {
                        if !ports.contains(&3000) && ports.is_empty() {
                            ports.push(3000);
                            let ev = Evidence::from_repo_file(
                                PathBuf::from("package.json"),
                                None,
                                "Default port 3000 inferred from Next.js configuration",
                            );
                            requirements.push(ProjectRequirement::new(
                                "port:3000",
                                RequirementKind::Port {
                                    port: 3000,
                                    service_hint: Some("nextjs".to_string()),
                                },
                                ev.clone(),
                            ));
                            evidence.push(ev);
                        } else if (has_dep("astro") || check_script_contains("astro"))
                            && !ports.contains(&4321)
                            && ports.is_empty()
                        {
                            ports.push(4321);
                            let ev = Evidence::from_repo_file(
                                PathBuf::from("package.json"),
                                None,
                                "Default port 4321 inferred from Astro configuration",
                            );
                            requirements.push(ProjectRequirement::new(
                                "port:4321",
                                RequirementKind::Port {
                                    port: 4321,
                                    service_hint: Some("astro".to_string()),
                                },
                                ev.clone(),
                            ));
                            evidence.push(ev);
                        }
                    }

                    // Database dependencies
                    if has_dep("pg") || has_dep("postgres") || has_dep("@prisma/client") {
                        let ev = Evidence::from_repo_file(
                            PathBuf::from("package.json"),
                            None,
                            "PostgreSQL client dependency detected in dependencies",
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
                            path: PathBuf::from("package.json"),
                            line: Some(e.line()),
                            detail: Some(e.to_string()),
                        },
                        Confidence::Confirmed,
                        format!("Syntax error in package.json (line {}): {}", e.line(), e),
                    ));
                }
            },
            Err(e) => {
                evidence.push(Evidence::new(
                    concord_core::evidence::EvidenceSource::RepositoryFile {
                        path: PathBuf::from("package.json"),
                        line: None,
                        detail: Some(e.to_string()),
                    },
                    Confidence::Confirmed,
                    format!("Failed to read package.json: {}", e),
                ));
            }
        }
    }

    NodeDiscovery {
        is_node,
        is_bun,
        package_managers,
        requirements,
        ports,
        scripts,
        workspaces,
        evidence,
    }
}

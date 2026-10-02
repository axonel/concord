pub mod autotools;
pub mod bootstrap;
pub mod cmake;
pub mod docker;
pub mod env;
pub mod go;
pub mod meson;
pub mod node;
pub mod python;
pub mod rust;
pub mod tool_versions;

use concord_core::error::Result;
use concord_core::evidence::Evidence;
use concord_core::ir::{ProjectComponent, ProjectManifest, ProjectRequirement, RequirementKind};
use concord_core::Confidence;
pub use concord_core::VersionConstraint;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum EntityKey {
    Runtime(String),
    PackageManager(String),
    DeveloperTool(String),
    BuildTool(String),
    CodeGenerator(String),
    Compiler(String),
    LanguagePackage(String, String),
    SystemLibrary(String),
    Service(String),
    Port(u16),
    EnvVar(String),
    Capability(String),
    Other(String),
}

fn get_entity_key(req: &ProjectRequirement) -> (EntityKey, Option<String>, Option<String>) {
    let base = match &req.kind {
        RequirementKind::Runtime { name, .. } => EntityKey::Runtime(name.to_lowercase()),
        RequirementKind::PackageManager { name, .. } => {
            EntityKey::PackageManager(name.to_lowercase())
        }
        RequirementKind::DeveloperTool { name, .. } => {
            EntityKey::DeveloperTool(name.to_lowercase())
        }
        RequirementKind::BuildTool { name, .. } => EntityKey::BuildTool(name.to_lowercase()),
        RequirementKind::CodeGenerator { name, .. } => {
            EntityKey::CodeGenerator(name.to_lowercase())
        }
        RequirementKind::Compiler { language, .. } => EntityKey::Compiler(language.to_lowercase()),
        RequirementKind::LanguagePackage {
            language, package, ..
        } => EntityKey::LanguagePackage(language.to_lowercase(), package.to_lowercase()),
        RequirementKind::SystemLibrary { name, .. } => {
            EntityKey::SystemLibrary(name.to_lowercase())
        }
        RequirementKind::Service { name, .. } => EntityKey::Service(name.to_lowercase()),
        RequirementKind::Port { port, .. } => EntityKey::Port(*port),
        RequirementKind::EnvVar { name, .. } => EntityKey::EnvVar(name.clone()),
        RequirementKind::AnyOf { capability, .. } => {
            EntityKey::Capability(capability.to_lowercase())
        }
        _ => EntityKey::Other(req.name.clone()),
    };
    let plat_key = req.platform.as_deref().map(|s| s.trim().to_lowercase());
    let arch_key = req.arch.as_deref().map(|s| {
        concord_core::arch::Architecture::normalize_from_str(s)
            .canonical_name()
            .to_string()
    });
    (base, plat_key, arch_key)
}

/// Consolidate requirements across all configuration sources deterministically.
/// Merges multi-source declarations (e.g. package.json + mise.toml),
/// mathematically intersects version constraints, preserves full provenance,
/// and flags contradictory constraints as explicit conflict requirements.
pub fn consolidate_requirements(requirements: Vec<ProjectRequirement>) -> Vec<ProjectRequirement> {
    let mut consolidated: Vec<ProjectRequirement> = Vec::new();
    let mut key_map: HashMap<(EntityKey, Option<String>, Option<String>), usize> = HashMap::new();
    let mut conflicts: Vec<ProjectRequirement> = Vec::new();

    for req in requirements {
        if matches!(req.kind, RequirementKind::Conflict { .. }) {
            conflicts.push(req);
            continue;
        }

        let key = get_entity_key(&req);
        if let Some(&idx) = key_map.get(&key) {
            let existing = &mut consolidated[idx];
            match (&existing.kind, &req.kind) {
                (
                    RequirementKind::Runtime {
                        name,
                        constraint: c1,
                    },
                    RequirementKind::Runtime { constraint: c2, .. },
                ) => match c1.intersect(c2) {
                    Ok(intersected) => {
                        existing.kind = RequirementKind::Runtime {
                            name: name.clone(),
                            constraint: intersected,
                        };
                        existing.additional_evidence.push(req.evidence);
                        existing.additional_evidence.extend(req.additional_evidence);
                    }
                    Err(err) => {
                        let p1 = match &existing.evidence.source {
                            concord_core::evidence::EvidenceSource::RepositoryFile {
                                path, ..
                            } => path.clone(),
                            _ => PathBuf::from("config1"),
                        };
                        let p2 = match &req.evidence.source {
                            concord_core::evidence::EvidenceSource::RepositoryFile {
                                path, ..
                            } => path.clone(),
                            _ => PathBuf::from("config2"),
                        };
                        let ev = Evidence::new(
                            concord_core::evidence::EvidenceSource::MultiSourceConflict {
                                summary: format!(
                                    "Contradictory {} version requirements: '{}' vs '{}'",
                                    name, c1, c2
                                ),
                                files: vec![p1.clone(), p2.clone()],
                            },
                            Confidence::Confirmed,
                            format!(
                                "Configuration conflict: {} ({}) conflicts with {} ({}): {}",
                                existing.evidence.description,
                                p1.display(),
                                req.evidence.description,
                                p2.display(),
                                err
                            ),
                        );
                        conflicts.push(ProjectRequirement::new(
                            format!("conflict:{}", name),
                            RequirementKind::Conflict {
                                target: name.clone(),
                                details: format!(
                                    "Contradictory {} version requirements: {} vs {}: {}",
                                    name, c1, c2, err
                                ),
                                competing_sources: vec![
                                    existing.evidence.description.clone(),
                                    req.evidence.description.clone(),
                                ],
                            },
                            ev,
                        ));
                    }
                },
                (
                    RequirementKind::PackageManager {
                        name,
                        constraint: c1,
                    },
                    RequirementKind::PackageManager { constraint: c2, .. },
                ) => {
                    let mut is_conflict = false;
                    let merged_constraint = match (c1, c2) {
                        (Some(v1), Some(v2)) => match v1.intersect(v2) {
                            Ok(intersected) => Some(intersected),
                            Err(err) => {
                                is_conflict = true;
                                let p1 = match &existing.evidence.source {
                                    concord_core::evidence::EvidenceSource::RepositoryFile {
                                        path,
                                        ..
                                    } => path.clone(),
                                    _ => PathBuf::from("config1"),
                                };
                                let p2 = match &req.evidence.source {
                                    concord_core::evidence::EvidenceSource::RepositoryFile {
                                        path,
                                        ..
                                    } => path.clone(),
                                    _ => PathBuf::from("config2"),
                                };
                                let ev = Evidence::new(
                                    concord_core::evidence::EvidenceSource::MultiSourceConflict {
                                        summary: format!(
                                            "Contradictory {} package manager version requirements: '{}' vs '{}'",
                                            name, v1, v2
                                        ),
                                        files: vec![p1.clone(), p2.clone()],
                                    },
                                    Confidence::Confirmed,
                                    format!(
                                        "Configuration conflict: {} ({}) conflicts with {} ({}): {}",
                                        existing.evidence.description,
                                        p1.display(),
                                        req.evidence.description,
                                        p2.display(),
                                        err
                                    ),
                                );
                                conflicts.push(ProjectRequirement::new(
                                    format!("conflict:{}", name),
                                    RequirementKind::Conflict {
                                        target: name.clone(),
                                        details: format!(
                                            "Contradictory {} version requirements: {} vs {}: {}",
                                            name, v1, v2, err
                                        ),
                                        competing_sources: vec![
                                            existing.evidence.description.clone(),
                                            req.evidence.description.clone(),
                                        ],
                                    },
                                    ev,
                                ));
                                Some(v1.clone())
                            }
                        },
                        (Some(v), None) | (None, Some(v)) => Some(v.clone()),
                        (None, None) => None,
                    };
                    if !is_conflict {
                        existing.kind = RequirementKind::PackageManager {
                            name: name.clone(),
                            constraint: merged_constraint,
                        };
                    }
                    existing.additional_evidence.push(req.evidence);
                    existing.additional_evidence.extend(req.additional_evidence);
                }
                (
                    RequirementKind::BuildTool {
                        name,
                        constraint: c1,
                        scope: s1,
                    },
                    RequirementKind::BuildTool {
                        constraint: c2,
                        scope: s2,
                        ..
                    },
                ) => {
                    let merged_scope = match (s1, s2) {
                        (concord_core::ir::ToolScope::RequiredForBuild, _)
                        | (_, concord_core::ir::ToolScope::RequiredForBuild) => {
                            concord_core::ir::ToolScope::RequiredForBuild
                        }
                        (concord_core::ir::ToolScope::RequiredForTask, _)
                        | (_, concord_core::ir::ToolScope::RequiredForTask) => {
                            concord_core::ir::ToolScope::RequiredForTask
                        }
                        _ => *s1,
                    };
                    let merged_constraint = match (c1, c2) {
                        (Some(v1), Some(v2)) => match v1.intersect(v2) {
                            Ok(intersected) => Some(intersected),
                            Err(_) => Some(v1.clone()),
                        },
                        (Some(v), None) | (None, Some(v)) => Some(v.clone()),
                        (None, None) => None,
                    };
                    existing.kind = RequirementKind::BuildTool {
                        name: name.clone(),
                        constraint: merged_constraint,
                        scope: merged_scope,
                    };
                    existing.additional_evidence.push(req.evidence);
                    existing.additional_evidence.extend(req.additional_evidence);
                }
                (
                    RequirementKind::CodeGenerator {
                        name,
                        constraint: c1,
                        scope: s1,
                    },
                    RequirementKind::CodeGenerator {
                        constraint: c2,
                        scope: s2,
                        ..
                    },
                ) => {
                    let merged_scope = match (s1, s2) {
                        (concord_core::ir::ToolScope::RequiredForBuild, _)
                        | (_, concord_core::ir::ToolScope::RequiredForBuild) => {
                            concord_core::ir::ToolScope::RequiredForBuild
                        }
                        (concord_core::ir::ToolScope::RequiredForTask, _)
                        | (_, concord_core::ir::ToolScope::RequiredForTask) => {
                            concord_core::ir::ToolScope::RequiredForTask
                        }
                        _ => *s1,
                    };
                    let merged_constraint = match (c1, c2) {
                        (Some(v1), Some(v2)) => match v1.intersect(v2) {
                            Ok(intersected) => Some(intersected),
                            Err(_) => Some(v1.clone()),
                        },
                        (Some(v), None) | (None, Some(v)) => Some(v.clone()),
                        (None, None) => None,
                    };
                    existing.kind = RequirementKind::CodeGenerator {
                        name: name.clone(),
                        constraint: merged_constraint,
                        scope: merged_scope,
                    };
                    existing.additional_evidence.push(req.evidence);
                    existing.additional_evidence.extend(req.additional_evidence);
                }
                (
                    RequirementKind::Compiler {
                        language,
                        min_standard: s1,
                        constraint: c1,
                    },
                    RequirementKind::Compiler {
                        min_standard: s2,
                        constraint: c2,
                        ..
                    },
                ) => {
                    let merged_standard = match (s1, s2) {
                        (Some(v1), Some(v2)) => {
                            if cmake::std_rank(language, v2) > cmake::std_rank(language, v1) {
                                Some(v2.clone())
                            } else {
                                Some(v1.clone())
                            }
                        }
                        (Some(v), None) | (None, Some(v)) => Some(v.clone()),
                        (None, None) => None,
                    };
                    let merged_constraint = match (c1, c2) {
                        (Some(v1), Some(v2)) => match v1.intersect(v2) {
                            Ok(inter) => Some(inter),
                            Err(_) => Some(v1.clone()),
                        },
                        (Some(v), None) | (None, Some(v)) => Some(v.clone()),
                        (None, None) => None,
                    };
                    existing.kind = RequirementKind::Compiler {
                        language: language.clone(),
                        min_standard: merged_standard,
                        constraint: merged_constraint,
                    };
                    existing.additional_evidence.push(req.evidence);
                    existing.additional_evidence.extend(req.additional_evidence);
                }
                (
                    RequirementKind::SystemLibrary {
                        name,
                        header: h1,
                        constraint: c1,
                        scope: s1,
                    },
                    RequirementKind::SystemLibrary {
                        header: h2,
                        constraint: c2,
                        scope: s2,
                        ..
                    },
                ) => {
                    let merged_scope = match (s1, s2) {
                        (concord_core::ir::ToolScope::RequiredForBuild, _)
                        | (_, concord_core::ir::ToolScope::RequiredForBuild) => {
                            concord_core::ir::ToolScope::RequiredForBuild
                        }
                        (concord_core::ir::ToolScope::RequiredForTask, _)
                        | (_, concord_core::ir::ToolScope::RequiredForTask) => {
                            concord_core::ir::ToolScope::RequiredForTask
                        }
                        _ => *s1,
                    };
                    let merged_header = h1.clone().or_else(|| h2.clone());
                    let merged_constraint = match (c1, c2) {
                        (Some(v1), Some(v2)) => match v1.intersect(v2) {
                            Ok(inter) => Some(inter),
                            Err(_) => Some(v1.clone()),
                        },
                        (Some(v), None) | (None, Some(v)) => Some(v.clone()),
                        (None, None) => None,
                    };
                    existing.kind = RequirementKind::SystemLibrary {
                        name: name.clone(),
                        header: merged_header,
                        constraint: merged_constraint,
                        scope: merged_scope,
                    };
                    existing.additional_evidence.push(req.evidence);
                    existing.additional_evidence.extend(req.additional_evidence);
                }
                (
                    RequirementKind::LanguagePackage {
                        language,
                        package,
                        constraint: c1,
                        scope: s1,
                    },
                    RequirementKind::LanguagePackage {
                        constraint: c2,
                        scope: s2,
                        ..
                    },
                ) => {
                    let merged_scope = match (s1, s2) {
                        (concord_core::ir::ToolScope::RequiredForBuild, _)
                        | (_, concord_core::ir::ToolScope::RequiredForBuild) => {
                            concord_core::ir::ToolScope::RequiredForBuild
                        }
                        _ => *s1,
                    };
                    let merged_constraint = match (c1, c2) {
                        (Some(v1), Some(v2)) => match v1.intersect(v2) {
                            Ok(inter) => Some(inter),
                            Err(_) => Some(v1.clone()),
                        },
                        (Some(v), None) | (None, Some(v)) => Some(v.clone()),
                        (None, None) => None,
                    };
                    existing.kind = RequirementKind::LanguagePackage {
                        language: language.clone(),
                        package: package.clone(),
                        constraint: merged_constraint,
                        scope: merged_scope,
                    };
                    existing.additional_evidence.push(req.evidence);
                    existing.additional_evidence.extend(req.additional_evidence);
                }
                (
                    RequirementKind::AnyOf {
                        capability,
                        alternatives: alts1,
                        scope: s1,
                    },
                    RequirementKind::AnyOf {
                        alternatives: alts2,
                        scope: s2,
                        ..
                    },
                ) => {
                    let mut merged_alts = alts1.clone();
                    for a2 in alts2 {
                        let k2 = get_entity_key(a2);
                        if !merged_alts.iter().any(|a1| get_entity_key(a1) == k2) {
                            merged_alts.push(a2.clone());
                        }
                    }
                    let merged_scope = match (s1, s2) {
                        (concord_core::ir::ToolScope::RequiredForProject, _)
                        | (_, concord_core::ir::ToolScope::RequiredForProject) => {
                            concord_core::ir::ToolScope::RequiredForProject
                        }
                        (concord_core::ir::ToolScope::RequiredForBuild, _)
                        | (_, concord_core::ir::ToolScope::RequiredForBuild) => {
                            concord_core::ir::ToolScope::RequiredForBuild
                        }
                        (concord_core::ir::ToolScope::RequiredForTask, _)
                        | (_, concord_core::ir::ToolScope::RequiredForTask) => {
                            concord_core::ir::ToolScope::RequiredForTask
                        }
                        _ => *s1,
                    };
                    existing.kind = RequirementKind::AnyOf {
                        capability: capability.clone(),
                        alternatives: merged_alts,
                        scope: merged_scope,
                    };
                    existing.additional_evidence.push(req.evidence);
                    existing.additional_evidence.extend(req.additional_evidence);
                }
                _ => {
                    existing.additional_evidence.push(req.evidence);
                    existing.additional_evidence.extend(req.additional_evidence);
                }
            }
        } else {
            let idx = consolidated.len();
            consolidated.push(req);
            key_map.insert(key, idx);
        }
    }

    consolidated.extend(conflicts);
    consolidated
}

struct DirAnalysis {
    languages: Vec<String>,
    package_managers: Vec<String>,
    requirements: Vec<ProjectRequirement>,
    declared_ports: Vec<u16>,
    env_vars: Vec<String>,
    env_var_specs: Vec<concord_core::ir::EnvVarSpec>,
    compose_projects: Vec<concord_core::ir::ComposeProjectSpec>,
    docker_used: bool,
    evidence: Vec<Evidence>,
}

/// Helper to analyze a specific directory for languages, requirements, and ports.
fn analyze_dir(dir: &Path) -> DirAnalysis {
    let node_disc = node::analyze_node(dir);
    let py_disc = python::analyze_python(dir);
    let rust_disc = rust::analyze_rust(dir);
    let go_disc = go::analyze_go(dir);
    let meson_disc = meson::analyze_meson(dir);
    let cmake_disc = cmake::analyze_cmake(dir);
    let autotools_disc = autotools::analyze_autotools(dir);
    let docker_disc = docker::analyze_docker(dir);
    let env_disc = env::analyze_env(dir);
    let tool_disc = tool_versions::analyze_tool_versions(dir);

    let mut languages = Vec::new();
    if node_disc.is_node {
        languages.push("javascript/typescript".to_string());
    }
    if node_disc.is_bun {
        languages.push("bun".to_string());
    }
    if py_disc.is_python {
        languages.push("python".to_string());
    }
    if rust_disc.is_rust {
        languages.push("rust".to_string());
    }
    if go_disc.is_go {
        languages.push("go".to_string());
    }
    languages.extend(meson_disc.languages);
    languages.extend(cmake_disc.languages);
    languages.extend(autotools_disc.languages);
    languages.sort();
    languages.dedup();

    let mut package_managers = node_disc.package_managers;
    package_managers.extend(py_disc.package_managers);
    package_managers.extend(rust_disc.package_managers);
    package_managers.extend(go_disc.package_managers);
    package_managers.sort();
    package_managers.dedup();

    let mut requirements = node_disc.requirements;
    requirements.extend(py_disc.requirements);
    requirements.extend(rust_disc.requirements);
    requirements.extend(go_disc.requirements);
    requirements.extend(meson_disc.requirements);
    requirements.extend(cmake_disc.requirements);
    requirements.extend(autotools_disc.requirements);
    requirements.extend(docker_disc.requirements);
    requirements.extend(env_disc.requirements);
    requirements.extend(tool_disc.requirements);

    let mut declared_ports = node_disc.ports;
    declared_ports.extend(py_disc.ports);
    declared_ports.extend(docker_disc.ports);
    declared_ports.extend(env_disc.ports);
    declared_ports.sort();
    declared_ports.dedup();

    let mut env_vars = docker_disc.env_vars;
    env_vars.extend(env_disc.env_vars);
    env_vars.sort();
    env_vars.dedup();

    let mut evidence = node_disc.evidence;
    evidence.extend(py_disc.evidence);
    evidence.extend(rust_disc.evidence);
    evidence.extend(go_disc.evidence);
    evidence.extend(meson_disc.evidence);
    evidence.extend(cmake_disc.evidence);
    evidence.extend(autotools_disc.evidence);
    evidence.extend(docker_disc.evidence);
    evidence.extend(env_disc.evidence);
    evidence.extend(tool_disc.evidence);

    let compose_projects = docker_disc.compose_projects;
    let docker_used = docker_disc.docker_used;
    let env_var_specs = env_disc.env_var_specs;

    DirAnalysis {
        languages,
        package_managers,
        requirements,
        declared_ports,
        env_vars,
        env_var_specs,
        compose_projects,
        docker_used,
        evidence,
    }
}

/// Analyze a project repository deterministically and produce a structured manifest.
pub fn analyze_project(root: &Path) -> Result<ProjectManifest> {
    if !root.exists() {
        return Err(concord_core::ConcordError::ProjectAnalysis(format!(
            "Target path '{}' does not exist",
            root.display()
        )));
    }
    if !root.is_dir() {
        return Err(concord_core::ConcordError::ProjectAnalysis(format!(
            "Target path '{}' is not a directory",
            root.display()
        )));
    }

    let root_buf = root
        .canonicalize()
        .map_err(concord_core::ConcordError::Io)?;

    // 1. Analyze root directory
    let root_analysis = analyze_dir(&root_buf);
    let mut languages = root_analysis.languages;
    let mut package_managers = root_analysis.package_managers;
    let mut requirements = root_analysis.requirements;
    let mut declared_ports = root_analysis.declared_ports;
    let mut env_vars = root_analysis.env_vars;
    let mut env_var_specs = root_analysis.env_var_specs;
    let mut compose_projects = root_analysis.compose_projects;
    let mut docker_used = root_analysis.docker_used;
    let mut evidence = root_analysis.evidence;

    // 2. Discover subcomponents (monorepo packages, frontend/backend directories, crates)
    let mut components = Vec::new();
    let candidate_subdirs = [
        "web", "frontend", "client", "ui", "backend", "server", "api", "services", "desktop",
        "mobile", "site",
    ];

    let mut candidate_paths = Vec::new();
    for sub in candidate_subdirs {
        let p = root_buf.join(sub);
        if p.is_dir() {
            candidate_paths.push((sub.to_string(), p));
        }
    }

    // Also inspect crates/* and packages/* and apps/*
    for group in ["crates", "packages", "apps"] {
        let group_dir = root_buf.join(group);
        if group_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&group_dir) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        let name = format!("{}/{}", group, entry.file_name().to_string_lossy());
                        candidate_paths.push((name, entry.path()));
                    }
                }
            }
        }
    }

    for (comp_name, comp_path) in candidate_paths {
        let c_analysis = analyze_dir(&comp_path);
        if !c_analysis.languages.is_empty()
            || !c_analysis.requirements.is_empty()
            || !c_analysis.declared_ports.is_empty()
            || !c_analysis.env_vars.is_empty()
            || !c_analysis.compose_projects.is_empty()
        {
            // Register component
            components.push(ProjectComponent {
                name: comp_name,
                path: comp_path,
                languages: c_analysis.languages.clone(),
                package_managers: c_analysis.package_managers.clone(),
                requirements: c_analysis.requirements.clone(),
                declared_ports: c_analysis.declared_ports.clone(),
                env_vars: c_analysis.env_vars.clone(),
            });

            // Merge up to root manifest
            languages.extend(c_analysis.languages);
            package_managers.extend(c_analysis.package_managers);
            requirements.extend(c_analysis.requirements);
            declared_ports.extend(c_analysis.declared_ports);
            env_vars.extend(c_analysis.env_vars);
            env_var_specs.extend(c_analysis.env_var_specs);
            compose_projects.extend(c_analysis.compose_projects);
            docker_used |= c_analysis.docker_used;
            evidence.extend(c_analysis.evidence);
        }
    }

    languages.sort();
    languages.dedup();
    package_managers.sort();
    package_managers.dedup();
    declared_ports.sort();
    declared_ports.dedup();
    env_vars.sort();
    env_vars.dedup();

    // 3. Consolidate requirements and detect conflicts across sources
    let requirements = consolidate_requirements(requirements);

    // 4. Statically analyze bootstrap scripts
    let bootstrap_actions = bootstrap::analyze_bootstrap(&root_buf);
    for action in &bootstrap_actions {
        evidence.push(Evidence::from_repo_file(
            action.script_path.clone(),
            None,
            format!("Bootstrap action discovered: {}", action.description),
        ));
    }

    // 5. Statically analyze build-system generation preconditions
    let build_system_generations = bootstrap::analyze_build_system_generation(&root_buf);
    for gen in &build_system_generations {
        evidence.push(Evidence::from_repo_file(
            gen.input_declaration.clone(),
            None,
            format!("Build system generation precondition: {}", gen.description),
        ));
    }

    let name = root_buf
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string());

    Ok(ProjectManifest {
        name,
        root_path: root_buf,
        languages,
        package_managers,
        requirements,
        declared_ports,
        env_vars,
        env_var_specs,
        components,
        compose_projects,
        bootstrap_actions,
        build_system_generations,
        docker_used,
        evidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_analyze_empty_directory() {
        let dir = tempdir().unwrap();
        let manifest = analyze_project(dir.path()).unwrap();
        assert!(manifest.languages.is_empty());
        assert!(manifest.package_managers.is_empty());
        assert!(manifest.requirements.is_empty());
        assert!(manifest.declared_ports.is_empty());
    }

    #[test]
    fn test_analyze_node_project() {
        let dir = tempdir().unwrap();
        let pkg_json = r#"{
            "name": "my-node-app",
            "engines": {
                "node": ">=20.0.0"
            },
            "scripts": {
                "dev": "vite --port 3000"
            }
        }"#;
        fs::write(dir.path().join("package.json"), pkg_json).unwrap();
        fs::write(dir.path().join("bun.lock"), "").unwrap();

        let manifest = analyze_project(dir.path()).unwrap();
        assert!(manifest
            .languages
            .contains(&"javascript/typescript".to_string()));
        assert!(manifest.languages.contains(&"bun".to_string()));
        assert!(manifest.package_managers.contains(&"bun".to_string()));
        assert!(manifest.declared_ports.contains(&3000));

        let node_req = manifest.requirements.iter().find(|r| r.name == "node");
        assert!(node_req.is_some());
    }

    #[test]
    fn test_analyze_rust_project() {
        let dir = tempdir().unwrap();
        let cargo_toml = r#"[package]
name = "my-rust-app"
version = "0.1.0"
edition = "2021"
rust-version = "1.85"

[dependencies]
sqlx = { version = "0.8", features = ["postgres"] }
"#;
        fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();

        let manifest = analyze_project(dir.path()).unwrap();
        assert!(manifest.languages.contains(&"rust".to_string()));
        assert!(manifest.package_managers.contains(&"cargo".to_string()));

        let rust_req = manifest.requirements.iter().find(|r| r.name == "rust");
        assert!(rust_req.is_some());
        if let Some(r) = rust_req {
            if let RequirementKind::Runtime { constraint, .. } = &r.kind {
                assert_eq!(
                    constraint,
                    &VersionConstraint::GreaterEqual("1.85".to_string())
                );
            } else {
                panic!("Expected Runtime requirement");
            }
        }

        let pg_req = manifest
            .requirements
            .iter()
            .find(|r| r.name == "postgresql");
        assert!(pg_req.is_some());
    }

    #[test]
    fn test_analyze_multi_component_project() {
        let dir = tempdir().unwrap();
        let web_dir = dir.path().join("web");
        fs::create_dir_all(&web_dir).unwrap();
        let pkg_json = r#"{
            "name": "web-frontend",
            "dependencies": { "vite": "^5.0.0" },
            "scripts": { "dev": "vite" }
        }"#;
        fs::write(web_dir.join("package.json"), pkg_json).unwrap();

        let backend_dir = dir.path().join("backend");
        fs::create_dir_all(&backend_dir).unwrap();
        let pyproject = r#"[project]
name = "backend-api"
requires-python = ">=3.11"
dependencies = ["fastapi>=0.110.0"]
"#;
        fs::write(backend_dir.join("pyproject.toml"), pyproject).unwrap();

        let manifest = analyze_project(dir.path()).unwrap();
        assert!(manifest
            .languages
            .contains(&"javascript/typescript".to_string()));
        assert!(manifest.languages.contains(&"python".to_string()));
        assert_eq!(manifest.components.len(), 2);
        assert!(manifest.declared_ports.contains(&5173)); // Vite default
        assert!(manifest.declared_ports.contains(&8000)); // FastAPI default
    }

    #[test]
    fn test_analyze_runtime_conflict() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".nvmrc"), "20\n").unwrap();
        let pkg_json = r#"{
            "name": "conflicted-app",
            "engines": {
                "node": ">=22.0.0"
            }
        }"#;
        fs::write(dir.path().join("package.json"), pkg_json).unwrap();

        let manifest = analyze_project(dir.path()).unwrap();
        let conflict = manifest
            .requirements
            .iter()
            .find(|r| r.name == "conflict:node");
        assert!(conflict.is_some(), "Expected conflict:node to be detected");
    }

    #[test]
    fn test_analyze_nonexistent_path() {
        let path = Path::new("/path/that/definitely/does/not/exist/9999");
        let result = analyze_project(path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("does not exist"));
    }

    #[test]
    fn test_analyze_file_not_dir() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("some_file.txt");
        fs::write(&file_path, "hello").unwrap();
        let result = analyze_project(&file_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("is not a directory"));
    }

    #[test]
    fn test_consolidate_multi_source_package_manager() {
        let dir = tempdir().unwrap();
        let pkg_json = r#"{
            "name": "immich-like-app",
            "packageManager": "pnpm@11.24.0",
            "engines": {
                "pnpm": ">=10.0.0"
            }
        }"#;
        fs::write(dir.path().join("package.json"), pkg_json).unwrap();
        let mise_toml = r#"
[tools]
pnpm = "11.24.0"
"#;
        fs::write(dir.path().join("mise.toml"), mise_toml).unwrap();

        let manifest = analyze_project(dir.path()).unwrap();
        let pnpm_reqs: Vec<&ProjectRequirement> = manifest
            .requirements
            .iter()
            .filter(|r| r.name == "pnpm")
            .collect();

        assert_eq!(
            pnpm_reqs.len(),
            1,
            "Should consolidate to exactly 1 pnpm requirement"
        );
        let pnpm_req = pnpm_reqs[0];
        if let RequirementKind::PackageManager { constraint, .. } = &pnpm_req.kind {
            assert_eq!(
                constraint.as_ref(),
                Some(&VersionConstraint::Exact("11.24.0".to_string()))
            );
        } else {
            panic!("Expected PackageManager requirement for pnpm");
        }
        assert!(
            !pnpm_req.additional_evidence.is_empty(),
            "Provenance from multiple sources should be preserved"
        );
    }
}

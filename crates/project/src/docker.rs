use concord_core::evidence::Evidence;
use concord_core::ir::{
    ComposeProjectSpec, ComposeServiceSpec, ProjectRequirement, RequirementKind,
};
use concord_core::VersionConstraint;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct DockerDiscovery {
    pub docker_used: bool,
    pub compose_projects: Vec<ComposeProjectSpec>,
    pub requirements: Vec<ProjectRequirement>,
    pub ports: Vec<u16>,
    pub env_vars: Vec<String>,
    pub evidence: Vec<Evidence>,
}

fn parse_base_image(image: &str) -> Option<(&str, Option<&str>)> {
    let base = image.split_whitespace().next()?.split('@').next()?;
    if let Some((name, tag)) = base.split_once(':') {
        Some((name, Some(tag)))
    } else {
        Some((base, None))
    }
}

/// Extract environment variable interpolations of the form ${VAR}, ${VAR:-default}, ${VAR-default}, ${VAR:?err}, etc.
pub fn extract_interpolations(text: &str) -> Vec<(String, Option<String>)> {
    let mut results = Vec::new();
    let mut cursor = 0;
    while let Some(start) = text[cursor..].find("${") {
        let abs_start = cursor + start;
        if let Some(end) = text[abs_start..].find('}') {
            let abs_end = abs_start + end;
            let inner = text[abs_start + 2..abs_end].trim();
            if let Some((var, default)) = inner.split_once(":-") {
                results.push((var.trim().to_string(), Some(default.trim().to_string())));
            } else if let Some((var, default)) = inner.split_once('-') {
                results.push((var.trim().to_string(), Some(default.trim().to_string())));
            } else if let Some((var, _err)) = inner.split_once(":?") {
                results.push((var.trim().to_string(), None));
            } else if let Some((var, _err)) = inner.split_once('?') {
                results.push((var.trim().to_string(), None));
            } else {
                results.push((inner.to_string(), None));
            }
            cursor = abs_end + 1;
        } else {
            break;
        }
    }
    results
}

fn collect_interpolations_from_yaml(
    value: &serde_yaml::Value,
    out: &mut Vec<(String, Option<String>)>,
) {
    match value {
        serde_yaml::Value::String(s) => {
            out.extend(extract_interpolations(s));
        }
        serde_yaml::Value::Sequence(seq) => {
            for v in seq {
                collect_interpolations_from_yaml(v, out);
            }
        }
        serde_yaml::Value::Mapping(map) => {
            for (_k, v) in map {
                collect_interpolations_from_yaml(v, out);
            }
        }
        _ => {}
    }
}

fn parse_env_file_keys(path: &Path) -> HashSet<String> {
    let mut keys = HashSet::new();
    if let Ok(content) = fs::read_to_string(path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((k, _)) = trimmed.split_once('=') {
                let clean_key = k.trim().trim_start_matches("export ").trim();
                if !clean_key.is_empty() {
                    keys.insert(clean_key.to_string());
                }
            }
        }
    }
    keys
}

pub fn infer_service_type(name: &str, image: Option<&str>) -> Option<String> {
    let name_lower = name.to_lowercase();
    let img_lower = image.map(|s| s.to_lowercase()).unwrap_or_default();

    if img_lower.contains("postgres")
        || name_lower.contains("postgres")
        || name_lower == "db"
        || (name_lower == "database" && (img_lower.contains("postgres") || img_lower.is_empty()))
    {
        Some("postgresql".to_string())
    } else if img_lower.contains("redis")
        || img_lower.contains("valkey")
        || name_lower.contains("redis")
        || name_lower.contains("valkey")
    {
        Some("redis".to_string())
    } else if img_lower.contains("mysql")
        || img_lower.contains("mariadb")
        || name_lower.contains("mysql")
        || name_lower.contains("mariadb")
    {
        Some("mysql".to_string())
    } else if img_lower.contains("mongo") || name_lower.contains("mongo") {
        Some("mongodb".to_string())
    } else if img_lower.contains("rabbitmq") || name_lower.contains("rabbitmq") {
        Some("rabbitmq".to_string())
    } else if img_lower.contains("kafka") || name_lower.contains("kafka") {
        Some("kafka".to_string())
    } else if img_lower.contains("elasticsearch")
        || img_lower.contains("opensearch")
        || name_lower.contains("elasticsearch")
    {
        Some("elasticsearch".to_string())
    } else {
        None
    }
}

fn find_env_template(missing_abs: &Path) -> Option<PathBuf> {
    let parent = missing_abs.parent()?;
    let file_name = missing_abs
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let candidates = if file_name == ".env" {
        vec![
            "example.env",
            ".env.example",
            ".env.template",
            ".env.sample",
            "env.example",
            ".env.default",
            ".example.env",
        ]
    } else {
        vec![
            "example.env",
            ".env.example",
            ".env.template",
            ".env.sample",
            "env.example",
        ]
    };

    for candidate in candidates {
        let cand_path = parent.join(candidate);
        if cand_path.is_file() {
            return Some(cand_path);
        }
    }
    None
}

fn parse_compose_file(compose_path: &Path, root: &Path) -> Option<ComposeProjectSpec> {
    let content = fs::read_to_string(compose_path).ok()?;
    let doc: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;
    let services_map = doc.get("services").and_then(|s| s.as_mapping())?;

    let compose_dir = compose_path.parent().unwrap_or(root);
    let rel_compose_path = compose_path
        .strip_prefix(root)
        .unwrap_or(compose_path)
        .to_path_buf();

    let project_name = doc
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            let p_name = compose_dir.file_name()?.to_string_lossy().to_string();
            if p_name == "docker" || p_name == ".docker" {
                root.file_name()
                    .map(|r| r.to_string_lossy().to_string())
                    .or(Some(p_name))
            } else {
                Some(p_name)
            }
        });

    let mut project_missing_env_files: Vec<PathBuf> = Vec::new();
    let mut project_referenced_env_files: Vec<PathBuf> = Vec::new();
    let mut project_env_templates: Vec<concord_core::ir::EnvFileTemplate> = Vec::new();
    let mut project_unresolved_vars: Vec<String> = Vec::new();
    let mut known_env_keys: HashSet<String> = HashSet::new();

    // Default .env in compose directory or repository root (per Compose spec)
    let default_env_path = compose_dir.join(".env");
    if default_env_path.is_file() {
        known_env_keys.extend(parse_env_file_keys(&default_env_path));
    } else {
        let root_env_path = root.join(".env");
        if root_env_path.is_file() {
            known_env_keys.extend(parse_env_file_keys(&root_env_path));
        }
    }

    // Check project-level env_file if any
    if let Some(ef_val) = doc.get("env_file") {
        let mut check_ef = |path_str: &str| {
            let rel_p = PathBuf::from(path_str);
            let abs_p = compose_dir.join(&rel_p);
            let proj_rel = abs_p.strip_prefix(root).unwrap_or(&abs_p).to_path_buf();
            if !project_referenced_env_files.contains(&proj_rel) {
                project_referenced_env_files.push(proj_rel.clone());
            }
            if abs_p.exists() {
                known_env_keys.extend(parse_env_file_keys(&abs_p));
            } else {
                if !project_missing_env_files.contains(&proj_rel) {
                    project_missing_env_files.push(proj_rel.clone());
                }
                if let Some(cand_abs) = find_env_template(&abs_p) {
                    let cand_rel = cand_abs
                        .strip_prefix(root)
                        .unwrap_or(&cand_abs)
                        .to_path_buf();
                    let match_entry = concord_core::ir::EnvFileTemplate {
                        missing_path: proj_rel,
                        template_path: cand_rel,
                    };
                    if !project_env_templates.contains(&match_entry) {
                        project_env_templates.push(match_entry);
                    }
                }
            }
        };

        if let Some(seq) = ef_val.as_sequence() {
            for item in seq {
                if let Some(s) = item.as_str() {
                    check_ef(s);
                }
            }
        } else if let Some(s) = ef_val.as_str() {
            check_ef(s);
        }
    }

    let mut services = Vec::new();

    for (svc_key, svc_val) in services_map {
        let name = match svc_key.as_str() {
            Some(n) => n.to_string(),
            None => continue,
        };

        let container_name = svc_val
            .get("container_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let image = svc_val
            .get("image")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let service_type = infer_service_type(&name, image.as_deref());

        let mut depends_on = Vec::new();
        if let Some(deps_val) = svc_val.get("depends_on") {
            if let Some(seq) = deps_val.as_sequence() {
                for item in seq {
                    if let Some(s) = item.as_str() {
                        depends_on.push(s.to_string());
                    }
                }
            } else if let Some(map) = deps_val.as_mapping() {
                for (k, _) in map {
                    if let Some(s) = k.as_str() {
                        depends_on.push(s.to_string());
                    }
                }
            }
        }

        let mut svc_env_files = Vec::new();
        if let Some(ef_val) = svc_val.get("env_file") {
            let mut check_ef = |path_str: &str| {
                let rel_p = PathBuf::from(path_str);
                let abs_p = compose_dir.join(&rel_p);
                let proj_rel = abs_p.strip_prefix(root).unwrap_or(&abs_p).to_path_buf();
                if !svc_env_files.contains(&proj_rel) {
                    svc_env_files.push(proj_rel.clone());
                }
                if !project_referenced_env_files.contains(&proj_rel) {
                    project_referenced_env_files.push(proj_rel.clone());
                }
                if abs_p.exists() {
                    known_env_keys.extend(parse_env_file_keys(&abs_p));
                } else {
                    if !project_missing_env_files.contains(&proj_rel) {
                        project_missing_env_files.push(proj_rel.clone());
                    }
                    if let Some(cand_abs) = find_env_template(&abs_p) {
                        let cand_rel = cand_abs
                            .strip_prefix(root)
                            .unwrap_or(&cand_abs)
                            .to_path_buf();
                        let match_entry = concord_core::ir::EnvFileTemplate {
                            missing_path: proj_rel,
                            template_path: cand_rel,
                        };
                        if !project_env_templates.contains(&match_entry) {
                            project_env_templates.push(match_entry);
                        }
                    }
                }
            };

            if let Some(seq) = ef_val.as_sequence() {
                for item in seq {
                    if let Some(s) = item.as_str() {
                        check_ef(s);
                    }
                }
            } else if let Some(s) = ef_val.as_str() {
                check_ef(s);
            }
        }

        let mut ports = Vec::new();
        if let Some(ports_val) = svc_val.get("ports") {
            if let Some(seq) = ports_val.as_sequence() {
                for item in seq {
                    let port_str = match item {
                        serde_yaml::Value::String(s) => s.clone(),
                        serde_yaml::Value::Number(n) => n.to_string(),
                        _ => continue,
                    };
                    let trimmed = port_str.trim();
                    let parts: Vec<&str> = trimmed.split(':').collect();
                    let host_port_str = if parts.len() >= 2 {
                        parts[parts.len() - 2]
                    } else {
                        parts[0]
                    };
                    if let Ok(p) = host_port_str.parse::<u16>() {
                        if p > 0 && !ports.contains(&p) {
                            ports.push(p);
                        }
                    }
                }
            }
        }

        let has_healthcheck = if let Some(hc) = svc_val.get("healthcheck") {
            let disabled = hc.get("disable").and_then(|v| v.as_bool()).unwrap_or(false);
            !disabled
        } else {
            false
        };

        let mut environment_vars = Vec::new();
        if let Some(env_val) = svc_val.get("environment") {
            if let Some(seq) = env_val.as_sequence() {
                for item in seq {
                    if let Some(s) = item.as_str() {
                        let k = s.split('=').next().unwrap_or(s).trim();
                        if !k.is_empty() {
                            environment_vars.push(k.to_string());
                        }
                    }
                }
            } else if let Some(map) = env_val.as_mapping() {
                for (k, _) in map {
                    if let Some(s) = k.as_str() {
                        if !s.is_empty() {
                            environment_vars.push(s.to_string());
                        }
                    }
                }
            }
        }

        let mut svc_interpolations = Vec::new();
        collect_interpolations_from_yaml(svc_val, &mut svc_interpolations);

        let mut unresolved_interpolations = Vec::new();
        for (var_name, default_opt) in svc_interpolations {
            if default_opt.is_none() {
                let in_host_env = std::env::var(&var_name).is_ok();
                let in_file_env = known_env_keys.contains(&var_name);
                if !in_host_env && !in_file_env {
                    if !unresolved_interpolations.contains(&var_name) {
                        unresolved_interpolations.push(var_name.clone());
                    }
                    if !project_unresolved_vars.contains(&var_name) {
                        project_unresolved_vars.push(var_name);
                    }
                }
            }
        }

        services.push(ComposeServiceSpec {
            name,
            container_name,
            image,
            service_type,
            depends_on,
            env_files: svc_env_files,
            ports,
            has_healthcheck,
            environment_vars,
            unresolved_interpolations,
        });
    }

    let can_instantiate =
        project_missing_env_files.is_empty() && project_unresolved_vars.is_empty();

    let mut directly_affected_services = Vec::new();
    let mut transitively_blocked_services = Vec::new();

    if !can_instantiate {
        for svc in &services {
            let has_missing_env = svc.env_files.iter().any(|ef| {
                project_missing_env_files.contains(ef)
                    || project_missing_env_files
                        .iter()
                        .any(|m| m.ends_with(ef) || ef.ends_with(m))
            });
            let has_unresolved = !svc.unresolved_interpolations.is_empty();
            if has_missing_env || has_unresolved {
                directly_affected_services.push(svc.name.clone());
            } else {
                transitively_blocked_services.push(svc.name.clone());
            }
        }
        directly_affected_services.sort();
        transitively_blocked_services.sort();
    }

    Some(ComposeProjectSpec {
        file_path: rel_compose_path,
        name: project_name,
        services,
        env_files: project_referenced_env_files,
        missing_env_files: project_missing_env_files,
        env_templates: project_env_templates,
        unresolved_env_vars: project_unresolved_vars,
        directly_affected_services,
        transitively_blocked_services,
        can_instantiate,
    })
}

fn compose_file_priority(path: &Path) -> u32 {
    let fname = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let path_str = path.to_string_lossy();

    // Deprioritize test / devcontainer / e2e compose files
    if path_str.contains("/e2e/")
        || path_str.contains("/.devcontainer/")
        || path_str.contains("/test/")
        || path_str.contains("/tests/")
    {
        return 100;
    }

    if fname.ends_with(".dev.yml") || fname.ends_with(".dev.yaml") {
        return 50;
    }
    if fname.ends_with(".prod.yml") || fname.ends_with(".prod.yaml") {
        return 60;
    }

    // Canonical docker-compose.yml or compose.yaml
    if fname == "docker-compose.yml" || fname == "compose.yaml" || fname == "compose.yml" {
        if path_str.contains("/docker/") || path_str.starts_with("docker/") {
            return 1;
        } else if path_str.contains("/deploy/") || path_str.starts_with("deploy/") {
            return 2;
        } else {
            return 0; // Root compose file is top priority
        }
    }

    20
}

pub fn analyze_docker(root: &Path) -> DockerDiscovery {
    let mut docker_used = false;
    let mut requirements = Vec::new();
    let mut ports = Vec::new();
    let mut env_vars = Vec::new();
    let mut evidence = Vec::new();
    let mut compose_projects = Vec::new();

    // 1. Find all Dockerfiles (Dockerfile, Dockerfile.*, Containerfile)
    let mut dockerfiles = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let fname = entry.file_name();
            let name_str = fname.to_string_lossy();
            if name_str == "Dockerfile"
                || name_str.starts_with("Dockerfile.")
                || name_str == "Containerfile"
            {
                dockerfiles.push(entry.path());
            }
        }
    }

    for df_path in dockerfiles {
        docker_used = true;
        let fname = df_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let ev = Evidence::from_repo_file(
            PathBuf::from(&fname),
            None,
            format!("Dockerfile detected ({})", fname),
        );
        evidence.push(ev);

        if let Ok(content) = fs::read_to_string(&df_path) {
            for (idx, line) in content.lines().enumerate() {
                let trimmed = line.trim();

                // Check FROM base image
                if let Some(rest) = trimmed.strip_prefix("FROM ") {
                    let image_str = rest.split_whitespace().next().unwrap_or("");
                    if let Some((name, tag)) = parse_base_image(image_str) {
                        let clean_name = name.split('/').next_back().unwrap_or(name);
                        match clean_name {
                            "node" | "bun" | "python" | "rust" | "golang" => {
                                let (rt_name, ver_constraint) = match clean_name {
                                    "golang" => {
                                        ("go", tag.map(|t| t.split('-').next().unwrap_or(t)))
                                    }
                                    other => (other, tag.map(|t| t.split('-').next().unwrap_or(t))),
                                };

                                let constraint = ver_constraint
                                    .map(|v| VersionConstraint::GreaterEqual(v.to_string()))
                                    .unwrap_or(VersionConstraint::Any);

                                let ev = Evidence::from_repo_file(
                                    PathBuf::from(&fname),
                                    Some(idx + 1),
                                    format!(
                                        "Runtime '{}' declared in Dockerfile FROM {}",
                                        rt_name, image_str
                                    ),
                                );
                                requirements.push(ProjectRequirement::new(
                                    rt_name,
                                    RequirementKind::Runtime {
                                        name: rt_name.to_string(),
                                        constraint,
                                    },
                                    ev.clone(),
                                ));
                                evidence.push(ev);
                            }
                            "postgres" | "postgresql" => {
                                let ev = Evidence::from_repo_file(
                                    PathBuf::from(&fname),
                                    Some(idx + 1),
                                    format!("PostgreSQL container image declared in {}", fname),
                                );
                                requirements.push(ProjectRequirement::new(
                                    "postgresql",
                                    RequirementKind::Service {
                                        name: "postgresql".to_string(),
                                        min_version: tag.map(|t| t.to_string()),
                                    },
                                    ev.clone(),
                                ));
                                evidence.push(ev);
                                if !ports.contains(&5432) {
                                    ports.push(5432);
                                }
                            }
                            _ => {}
                        }
                    }
                }

                // Check EXPOSE
                if let Some(rest) = trimmed.strip_prefix("EXPOSE ") {
                    for part in rest.split_whitespace() {
                        let clean = part.split('/').next().unwrap_or(part);
                        if let Ok(port) = clean.parse::<u16>() {
                            if !ports.contains(&port) {
                                ports.push(port);
                                let ev = Evidence::from_repo_file(
                                    PathBuf::from(&fname),
                                    Some(idx + 1),
                                    format!("Port {} exposed in {}", port, fname),
                                );
                                requirements.push(ProjectRequirement::new(
                                    format!("port:{}", port),
                                    RequirementKind::Port {
                                        port,
                                        service_hint: Some("docker".to_string()),
                                    },
                                    ev.clone(),
                                ));
                                evidence.push(ev);
                            }
                        }
                    }
                }

                // Check ENV
                if let Some(rest) = trimmed.strip_prefix("ENV ") {
                    let key = rest.split(['=', ' ']).next().unwrap_or("").trim();
                    if !key.is_empty() && !env_vars.contains(&key.to_string()) {
                        env_vars.push(key.to_string());
                    }
                }
            }
        }
    }

    // 2. Discover Docker Compose files recursively up to depth 3
    let mut compose_paths = Vec::new();
    for entry in WalkDir::new(root)
        .max_depth(3)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            if name.starts_with('.') && name != ".docker" && name != ".devcontainer" {
                return false;
            }
            if matches!(
                name.as_ref(),
                "node_modules" | "target" | "vendor" | ".next" | "dist" | "build" | "cache" | "tmp"
            ) {
                return false;
            }
            true
        })
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let name = entry.file_name().to_string_lossy();
            if (name.contains("compose") || name.starts_with("docker-compose"))
                && (name.ends_with(".yml") || name.ends_with(".yaml"))
            {
                compose_paths.push(entry.into_path());
            }
        }
    }

    // Sort compose paths by priority (canonical files first)
    compose_paths.sort_by_key(|p| compose_file_priority(p));

    for compose_path in compose_paths {
        if let Some(project_spec) = parse_compose_file(&compose_path, root) {
            docker_used = true;
            let display_name = project_spec.file_path.display().to_string();
            let ev = Evidence::from_repo_file(
                project_spec.file_path.clone(),
                None,
                format!("Docker Compose configuration detected ({})", display_name),
            );
            evidence.push(ev);

            // Register services and ports
            for svc in &project_spec.services {
                if let Some(ref stype) = svc.service_type {
                    let ev_svc = Evidence::from_repo_file(
                        project_spec.file_path.clone(),
                        None,
                        format!(
                            "Compose service '{}' of type '{}' declared in {}",
                            svc.name, stype, display_name
                        ),
                    );
                    requirements.push(ProjectRequirement::new(
                        stype,
                        RequirementKind::Service {
                            name: stype.clone(),
                            min_version: None,
                        },
                        ev_svc.clone(),
                    ));
                    evidence.push(ev_svc);
                }

                for port in &svc.ports {
                    if !ports.contains(port) {
                        ports.push(*port);
                        let ev_port = Evidence::from_repo_file(
                            project_spec.file_path.clone(),
                            None,
                            format!(
                                "Port {} mapped by service '{}' in {}",
                                port, svc.name, display_name
                            ),
                        );
                        requirements.push(ProjectRequirement::new(
                            format!("port:{}", port),
                            RequirementKind::Port {
                                port: *port,
                                service_hint: Some(format!("docker-compose:{}", svc.name)),
                            },
                            ev_port.clone(),
                        ));
                        evidence.push(ev_port);
                    }
                }

                for var in &svc.environment_vars {
                    if !env_vars.contains(var) {
                        env_vars.push(var.clone());
                    }
                }
            }

            for tmpl in &project_spec.env_templates {
                let ev_tmpl = Evidence::from_repo_file(
                    tmpl.template_path.clone(),
                    None,
                    format!(
                        "Configuration template found for missing environment file '{}'",
                        tmpl.missing_path.display()
                    ),
                );
                evidence.push(ev_tmpl);
            }

            compose_projects.push(project_spec);
        }
    }

    if docker_used {
        let ev = Evidence::from_repo_file(
            PathBuf::from("Dockerfile/compose"),
            None,
            "Docker service required by project containers",
        );
        requirements.push(ProjectRequirement::new(
            "docker",
            RequirementKind::Service {
                name: "docker".to_string(),
                min_version: None,
            },
            ev.clone(),
        ));
        evidence.push(ev);
    }

    DockerDiscovery {
        docker_used,
        compose_projects,
        requirements,
        ports,
        env_vars,
        evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_extract_interpolations() {
        let text =
            "POSTGRES_PASSWORD: ${DB_PASSWORD} and ${IMMICH_VERSION:-release} and ${OPT:-val}";
        let res = extract_interpolations(text);
        assert_eq!(res.len(), 3);
        assert_eq!(res[0], ("DB_PASSWORD".to_string(), None));
        assert_eq!(
            res[1],
            ("IMMICH_VERSION".to_string(), Some("release".to_string()))
        );
        assert_eq!(res[2], ("OPT".to_string(), Some("val".to_string())));
    }

    #[test]
    fn test_parse_compose_file_unresolved() {
        let dir = tempdir().unwrap();
        let docker_dir = dir.path().join("docker");
        fs::create_dir_all(&docker_dir).unwrap();

        let compose_content = r#"
name: testproj
services:
  database:
    container_name: test_postgres
    image: postgres:16
    env_file:
      - .env
    environment:
      POSTGRES_PASSWORD: ${DB_PASSWORD}
"#;
        let compose_path = docker_dir.join("docker-compose.yml");
        fs::write(&compose_path, compose_content).unwrap();

        let disc = analyze_docker(dir.path());
        assert!(disc.docker_used);
        assert_eq!(disc.compose_projects.len(), 1);

        let proj = &disc.compose_projects[0];
        assert_eq!(proj.name.as_deref(), Some("testproj"));
        assert!(!proj.can_instantiate);
        assert!(proj
            .missing_env_files
            .contains(&PathBuf::from("docker/.env")));
        assert!(proj
            .unresolved_env_vars
            .contains(&"DB_PASSWORD".to_string()));

        let svc = proj.services.iter().find(|s| s.name == "database").unwrap();
        assert_eq!(svc.service_type.as_deref(), Some("postgresql"));
        assert_eq!(svc.container_name.as_deref(), Some("test_postgres"));
    }

    #[test]
    fn test_parse_compose_file_resolved() {
        let dir = tempdir().unwrap();
        let docker_dir = dir.path().join("docker");
        fs::create_dir_all(&docker_dir).unwrap();

        let compose_content = r#"
name: testproj
services:
  database:
    container_name: test_postgres
    image: postgres:16
    env_file:
      - .env
    environment:
      POSTGRES_PASSWORD: ${DB_PASSWORD}
"#;
        let compose_path = docker_dir.join("docker-compose.yml");
        fs::write(&compose_path, compose_content).unwrap();
        fs::write(docker_dir.join(".env"), "DB_PASSWORD=secret\n").unwrap();

        let disc = analyze_docker(dir.path());
        assert_eq!(disc.compose_projects.len(), 1);
        let proj = &disc.compose_projects[0];
        assert!(
            proj.missing_env_files.is_empty(),
            "Expected no missing env files"
        );
        assert!(
            proj.unresolved_env_vars.is_empty(),
            "Expected no unresolved env vars"
        );
        assert!(proj.can_instantiate);
    }
}

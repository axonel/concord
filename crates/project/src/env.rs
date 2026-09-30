use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use concord_core::evidence::Evidence;
use concord_core::ir::{EnvVarCategory, EnvVarSpec, ProjectRequirement, RequirementKind};
use concord_core::Confidence;

pub struct EnvDiscovery {
    pub env_vars: Vec<String>,
    pub env_var_specs: Vec<EnvVarSpec>,
    pub ports: Vec<u16>,
    pub requirements: Vec<ProjectRequirement>,
    pub evidence: Vec<Evidence>,
}

/// Helper to parse key=value lines from an environment file.
/// Returns: (line_num, key, value, is_comment_required, is_quoted_empty)
fn parse_env_lines(content: &str) -> Vec<(usize, String, String, bool, bool)> {
    let mut entries = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let is_comment_required = line.to_lowercase().contains("required");

        if let Some((key, val_with_comment)) = trimmed.split_once('=') {
            let key = key.trim().to_string();
            // Separate value from inline comment if present
            let val_raw = val_with_comment
                .split('#')
                .next()
                .unwrap_or(val_with_comment)
                .trim();
            let is_quoted_empty = val_raw == "\"\"" || val_raw == "''";
            let val = val_raw.trim_matches('"').trim_matches('\'').to_string();
            entries.push((idx + 1, key, val, is_comment_required, is_quoted_empty));
        }
    }
    entries
}

fn is_secret_credential_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    upper.contains("SECRET")
        || upper.contains("TOKEN")
        || upper.contains("PASSWORD")
        || upper.contains("API_KEY")
        || upper.contains("AUTH_KEY")
}

/// Parse PostgreSQL connection URI to extract host and port.
/// Format: postgresql://[user[:password]@][netloc][:port][/dbname]
fn parse_postgres_url(url: &str) -> (Option<String>, Option<u16>) {
    let without_proto = if let Some(rest) = url.strip_prefix("postgresql://") {
        rest
    } else if let Some(rest) = url.strip_prefix("postgres://") {
        rest
    } else {
        return (None, None);
    };

    // Remove path / query / fragment
    let host_part = without_proto.split('/').next().unwrap_or(without_proto);
    // Remove user:pass@ if present
    let host_port = host_part.split('@').next_back().unwrap_or(host_part);

    if let Some((host, port_str)) = host_port.split_once(':') {
        let host = host.trim().to_string();
        let port = port_str.parse::<u16>().ok();
        (if host.is_empty() { None } else { Some(host) }, port)
    } else {
        let host = host_port.trim().to_string();
        (if host.is_empty() { None } else { Some(host) }, Some(5432))
    }
}

pub fn analyze_env(root: &Path) -> EnvDiscovery {
    let mut env_vars = Vec::new();
    let mut env_var_specs = Vec::new();
    let mut ports = Vec::new();
    let mut requirements = Vec::new();
    let mut evidence = Vec::new();

    // 1. Check for local configuration files (.env, .env.local)
    let mut local_env_keys = HashSet::new();
    for local_file in [".env", ".env.local"] {
        let local_path = root.join(local_file);
        if local_path.exists() {
            if let Ok(content) = fs::read_to_string(&local_path) {
                let ev = Evidence::from_repo_file(
                    PathBuf::from(local_file),
                    None,
                    format!("Local configuration file present ({})", local_file),
                );
                evidence.push(ev);

                for (_, key, val, _, _) in parse_env_lines(&content) {
                    if !val.is_empty() {
                        local_env_keys.insert(key.clone());
                        env_var_specs.push(EnvVarSpec {
                            name: key.clone(),
                            category: EnvVarCategory::ConfiguredLocal,
                            default_value: Some(val),
                            declared_source: Some(PathBuf::from(local_file)),
                        });
                        if !env_vars.contains(&key) {
                            env_vars.push(key);
                        }
                    }
                }
            }
        }
    }

    // 2. Candidate template files
    let candidate_files = [
        ".env.example",
        ".env.sample",
        ".env.template",
        "server.env.example",
    ];

    for filename in candidate_files {
        let path = root.join(filename);
        if path.exists() {
            let ev = Evidence::from_repo_file(
                PathBuf::from(filename),
                None,
                format!("Environment template file detected ({})", filename),
            );
            evidence.push(ev);

            if let Ok(content) = fs::read_to_string(&path) {
                for (line_num, key, val, is_required_comment, is_quoted_empty) in
                    parse_env_lines(&content)
                {
                    if !env_vars.contains(&key) {
                        env_vars.push(key.clone());
                    }

                    // Check if already configured locally
                    let is_configured_local = local_env_keys.contains(&key);

                    // Determine variable category and whether it is strictly required
                    let (category, is_strictly_required) = if is_configured_local {
                        (EnvVarCategory::ConfiguredLocal, false)
                    } else if is_required_comment {
                        (EnvVarCategory::Required, true)
                    } else if is_quoted_empty {
                        (EnvVarCategory::IntentionallyEmpty, false)
                    } else if val.is_empty() {
                        if is_secret_credential_key(&key) {
                            (EnvVarCategory::Required, true)
                        } else {
                            (EnvVarCategory::Optional, false)
                        }
                    } else {
                        (EnvVarCategory::OptionalWithDefault, false)
                    };

                    env_var_specs.push(EnvVarSpec {
                        name: key.clone(),
                        category,
                        default_value: if is_quoted_empty {
                            Some("".to_string())
                        } else if val.is_empty() {
                            None
                        } else {
                            Some(val.clone())
                        },
                        declared_source: Some(PathBuf::from(filename)),
                    });

                    // Port extraction: PORT=3000
                    if key.eq_ignore_ascii_case("PORT") {
                        if let Ok(p) = val.parse::<u16>() {
                            if !ports.contains(&p) && p > 0 {
                                ports.push(p);
                                let ev = Evidence::from_repo_file(
                                    PathBuf::from(filename),
                                    Some(line_num),
                                    format!("Port {} declared via {}={}", p, key, val),
                                );
                                requirements.push(ProjectRequirement::new(
                                    format!("port:{}", p),
                                    RequirementKind::Port {
                                        port: p,
                                        service_hint: Some("environment".to_string()),
                                    },
                                    ev.clone(),
                                ));
                                evidence.push(ev);
                            }
                        }
                    }

                    // Database connection string extraction
                    if (key.contains("DATABASE") || key.contains("POSTGRES"))
                        && val.contains("postgres")
                    {
                        let (_host, parsed_port) = parse_postgres_url(&val);
                        let db_port = parsed_port.unwrap_or(5432);

                        let ev = Evidence::from_repo_file(
                            PathBuf::from(filename),
                            Some(line_num),
                            format!(
                                "PostgreSQL database requirement inferred from {} (port {})",
                                key, db_port
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

                        if !ports.contains(&db_port) {
                            ports.push(db_port);
                        }
                    }

                    // Only emit ProjectRequirement for strictly required env vars
                    if is_strictly_required {
                        let ev = Evidence::new(
                            concord_core::evidence::EvidenceSource::RepositoryFile {
                                path: PathBuf::from(filename),
                                line: Some(line_num),
                                detail: Some(format!("Declared in {}", filename)),
                            },
                            Confidence::High,
                            format!("Required environment variable '{}' has no default value and must be supplied", key),
                        );
                        requirements.push(ProjectRequirement::new(
                            key.clone(),
                            RequirementKind::EnvVar {
                                name: key,
                                default_value: None,
                                required: true,
                            },
                            ev.clone(),
                        ));
                        evidence.push(ev);
                    }
                }
            }
        }
    }

    EnvDiscovery {
        env_vars,
        env_var_specs,
        ports,
        requirements,
        evidence,
    }
}

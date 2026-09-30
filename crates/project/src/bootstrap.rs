use concord_core::ir::BootstrapAction;
use std::fs;
use std::path::{Path, PathBuf};

/// Common bootstrap / setup script files to inspect.
const BOOTSTRAP_SCRIPTS: &[&str] = &[
    "setup.sh",
    "bootstrap.sh",
    "init.sh",
    "Makefile",
    "justfile",
];

/// Statically inspects repository bootstrap scripts for deterministic environment setup actions.
/// Zero code execution — purely static pattern recognition.
pub fn analyze_bootstrap(root: &Path) -> Vec<BootstrapAction> {
    let mut actions = Vec::new();

    for script_name in BOOTSTRAP_SCRIPTS {
        let script_path = root.join(script_name);
        if !script_path.is_file() {
            continue;
        }

        let content = match fs::read_to_string(&script_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // 1. Direct copy patterns: cp .env.example .env or cp -n ...
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                continue;
            }

            if trimmed.contains("cp ") || trimmed.contains("copy_env_file") {
                // Check for common template -> env target patterns
                if (trimmed.contains(".env.example") || trimmed.contains("example.env"))
                    && trimmed.contains(".env")
                {
                    // Extract possible path tokens
                    let tokens: Vec<&str> = trimmed
                        .split([' ', '\t', '"', '\''])
                        .map(|t| t.trim())
                        .filter(|t| !t.is_empty())
                        .collect();

                    let mut found_src = None;
                    let mut found_dst = None;

                    for token in tokens {
                        let clean = token.trim_start_matches("./");
                        if clean.contains('$') {
                            continue;
                        }
                        if clean.ends_with(".env.example") || clean.ends_with("example.env") {
                            found_src = Some(clean);
                        } else if clean.ends_with(".env") && !clean.ends_with(".example") {
                            found_dst = Some(clean);
                        }
                    }

                    if let (Some(src), Some(dst)) = (found_src, found_dst) {
                        let action = BootstrapAction {
                            script_path: PathBuf::from(script_name),
                            action_type: "copy_template".to_string(),
                            source_template: PathBuf::from(src),
                            target_file: PathBuf::from(dst),
                            description: format!("{} copies {} to {}", script_name, src, dst),
                        };
                        if !actions.contains(&action) {
                            actions.push(action);
                        }
                    }
                }
            }
        }

        // 2. Loop-based copy patterns in shell scripts (e.g. for service in ...; cp .../${service}/.env.example)
        if content.contains(".env.example") && content.contains(".env") && content.contains("for ")
        {
            // Check if root template copy was found
            let root_action = BootstrapAction {
                script_path: PathBuf::from(script_name),
                action_type: "copy_template".to_string(),
                source_template: PathBuf::from(".env.example"),
                target_file: PathBuf::from(".env"),
                description: format!("{} copies .env.example to .env", script_name),
            };
            if root.join(".env.example").is_file() && !actions.contains(&root_action) {
                actions.push(root_action);
            }

            // Check if apps/ or packages/ subdirectories have .env.example
            for sub in ["apps", "packages", "services"] {
                let sub_dir = root.join(sub);
                if let Ok(entries) = fs::read_dir(sub_dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_dir() {
                            let example_p = p.join(".env.example");
                            if example_p.is_file() {
                                let rel_src = example_p
                                    .strip_prefix(root)
                                    .unwrap_or(&example_p)
                                    .to_path_buf();
                                let rel_dst = p
                                    .join(".env")
                                    .strip_prefix(root)
                                    .unwrap_or(&p.join(".env"))
                                    .to_path_buf();
                                let action = BootstrapAction {
                                    script_path: PathBuf::from(script_name),
                                    action_type: "copy_template".to_string(),
                                    source_template: rel_src.clone(),
                                    target_file: rel_dst.clone(),
                                    description: format!(
                                        "{} copies {} to {}",
                                        script_name,
                                        rel_src.display(),
                                        rel_dst.display()
                                    ),
                                };
                                if !actions.contains(&action) {
                                    actions.push(action);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_analyze_bootstrap_cp_pattern() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("setup.sh");
        fs::write(
            &script,
            "#!/bin/bash\ncp .env.example .env\ncp apps/api/.env.example apps/api/.env\n",
        )
        .unwrap();

        let actions = analyze_bootstrap(dir.path());
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].target_file, PathBuf::from(".env"));
        assert_eq!(actions[1].target_file, PathBuf::from("apps/api/.env"));
    }
}

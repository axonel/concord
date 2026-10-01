use concord_core::ir::{BootstrapAction, BuildSystemGeneration};
use std::fs;
use std::path::{Path, PathBuf};

/// Common bootstrap / setup script files to inspect.
const BOOTSTRAP_SCRIPTS: &[&str] = &[
    "autogen.sh",
    "bootstrap.sh",
    "bootstrap",
    "init.sh",
    "setup.sh",
    "Makefile",
    "justfile",
];

/// Statically inspects repository bootstrap scripts for deterministic environment setup actions.
/// Zero code execution — purely static pattern recognition.
pub fn analyze_bootstrap(root: &Path) -> Vec<BootstrapAction> {
    let mut actions = Vec::new();

    // 1. Check for build system bootstrap script actions (e.g. autogen.sh generating configure from configure.ac)
    let has_configure_ac = root.join("configure.ac").is_file();
    let has_configure_in = root.join("configure.in").is_file();
    if has_configure_ac || has_configure_in {
        let input_file = if has_configure_ac {
            PathBuf::from("configure.ac")
        } else {
            PathBuf::from("configure.in")
        };
        for script_name in &["autogen.sh", "bootstrap.sh", "bootstrap", "bootstrap.py"] {
            let script_path = root.join(script_name);
            if script_path.is_file() {
                let action = BootstrapAction {
                    script_path: PathBuf::from(script_name),
                    action_type: "generate_build_system".to_string(),
                    source_template: input_file.clone(),
                    target_file: PathBuf::from("configure"),
                    description: format!(
                        "{} generates configure from {}",
                        script_name,
                        input_file.display()
                    ),
                };
                if !actions.contains(&action) {
                    actions.push(action);
                }
                break;
            }
        }
    }

    for script_name in BOOTSTRAP_SCRIPTS {
        let script_path = root.join(script_name);
        if !script_path.is_file() {
            continue;
        }

        let content = match fs::read_to_string(&script_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // 2. Direct copy patterns: cp .env.example .env or cp -n ...
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

            // 3. Generic custom generator invocations in script: <tool> <input> ... -o <output>
            if trimmed.contains(" -o ") {
                let tokens: Vec<&str> = trimmed
                    .split_whitespace()
                    .map(|t| t.trim_matches(['"', '\'']))
                    .filter(|t| !t.is_empty())
                    .collect();
                if let Some(o_idx) = tokens.iter().position(|&t| t == "-o") {
                    if o_idx + 1 < tokens.len() && !tokens.is_empty() {
                        let output = tokens[o_idx + 1];
                        let tool = tokens[0].trim_start_matches("./");
                        for token in &tokens[1..o_idx] {
                            let clean_token = token.trim_start_matches("./");
                            if root.join(clean_token).is_file() {
                                let action = BootstrapAction {
                                    script_path: PathBuf::from(script_name),
                                    action_type: "generate_build_system".to_string(),
                                    source_template: PathBuf::from(clean_token),
                                    target_file: PathBuf::from(output),
                                    description: format!(
                                        "{} executes '{}' on '{}' to generate '{}'",
                                        script_name, tool, clean_token, output
                                    ),
                                };
                                if !actions.contains(&action) {
                                    actions.push(action);
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }

        // 4. Loop-based copy patterns in shell scripts
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

/// Statically discovers build-system generation preconditions in the repository.
/// Captures dependencies where an input declaration requires a generator to produce
/// downstream build configuration files.
pub fn analyze_build_system_generation(root: &Path) -> Vec<BuildSystemGeneration> {
    let mut generations = Vec::new();

    // 1. Standard Autotools declaration (configure.ac / configure.in -> autoreconf -> configure)
    let has_configure_ac = root.join("configure.ac").is_file();
    let has_configure_in = root.join("configure.in").is_file();
    if has_configure_ac || has_configure_in {
        let input_file = if has_configure_ac {
            PathBuf::from("configure.ac")
        } else {
            PathBuf::from("configure.in")
        };

        let mut bootstrap_script = None;
        for candidate in &["autogen.sh", "bootstrap.sh", "bootstrap", "bootstrap.py"] {
            if root.join(candidate).is_file() {
                bootstrap_script = Some(PathBuf::from(candidate));
                break;
            }
        }

        generations.push(BuildSystemGeneration {
            input_declaration: input_file.clone(),
            generator_tool: "autoreconf".to_string(),
            version_constraint: None,
            generated_artifact: PathBuf::from("configure"),
            downstream_build_system: "configure".to_string(),
            bootstrap_script,
            description: format!(
                "Autotools declaration '{}' requires 'autoreconf' to generate 'configure'",
                input_file.display()
            ),
        });
    }

    // 2. Automake declaration (Makefile.am -> automake -> Makefile.in)
    if root.join("Makefile.am").is_file() {
        let mut bootstrap_script = None;
        for candidate in &["autogen.sh", "bootstrap.sh", "bootstrap"] {
            if root.join(candidate).is_file() {
                bootstrap_script = Some(PathBuf::from(candidate));
                break;
            }
        }

        generations.push(BuildSystemGeneration {
            input_declaration: PathBuf::from("Makefile.am"),
            generator_tool: "automake".to_string(),
            version_constraint: None,
            generated_artifact: PathBuf::from("Makefile.in"),
            downstream_build_system: "make".to_string(),
            bootstrap_script,
            description:
                "Automake declaration 'Makefile.am' requires 'automake' to generate 'Makefile.in'"
                    .to_string(),
        });
    }

    // 3. CMake template declaration (CMakeLists.txt.in -> cmake -> CMakeLists.txt)
    if root.join("CMakeLists.txt.in").is_file() {
        let mut bootstrap_script = None;
        for candidate in &["configure.sh", "bootstrap.sh", "setup.sh"] {
            if root.join(candidate).is_file() {
                bootstrap_script = Some(PathBuf::from(candidate));
                break;
            }
        }

        generations.push(BuildSystemGeneration {
            input_declaration: PathBuf::from("CMakeLists.txt.in"),
            generator_tool: "cmake".to_string(),
            version_constraint: None,
            generated_artifact: PathBuf::from("CMakeLists.txt"),
            downstream_build_system: "cmake".to_string(),
            bootstrap_script,
            description:
                "CMake template 'CMakeLists.txt.in' requires generation to produce 'CMakeLists.txt'"
                    .to_string(),
        });
    }

    // 4. Meson template declaration (meson.build.in -> meson -> meson.build)
    if root.join("meson.build.in").is_file() {
        generations.push(BuildSystemGeneration {
            input_declaration: PathBuf::from("meson.build.in"),
            generator_tool: "meson".to_string(),
            version_constraint: None,
            generated_artifact: PathBuf::from("meson.build"),
            downstream_build_system: "meson".to_string(),
            bootstrap_script: None,
            description:
                "Meson template 'meson.build.in' requires generation to produce 'meson.build'"
                    .to_string(),
        });
    }

    // 5. Generic bootstrap script inspection for custom / anonymous generation steps
    for script_name in BOOTSTRAP_SCRIPTS {
        let script_path = root.join(script_name);
        if !script_path.is_file() {
            continue;
        }

        let content = match fs::read_to_string(&script_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                continue;
            }

            // Pattern: <tool> <input> ... -o <output>
            if trimmed.contains(" -o ") {
                let tokens: Vec<&str> = trimmed
                    .split_whitespace()
                    .map(|t| t.trim_matches(['"', '\'']))
                    .filter(|t| !t.is_empty())
                    .collect();

                if let Some(o_idx) = tokens.iter().position(|&t| t == "-o") {
                    if o_idx + 1 < tokens.len() && !tokens.is_empty() {
                        let output = tokens[o_idx + 1];
                        let tool = tokens[0].trim_start_matches("./");
                        for token in &tokens[1..o_idx] {
                            let clean_token = token.trim_start_matches("./");
                            if root.join(clean_token).is_file() {
                                let downstream =
                                    if output.ends_with(".mk") || output.contains("Makefile") {
                                        "make"
                                    } else if output == "CMakeLists.txt" {
                                        "cmake"
                                    } else if output == "configure" {
                                        "configure"
                                    } else {
                                        "build"
                                    };

                                let gen = BuildSystemGeneration {
                                    input_declaration: PathBuf::from(clean_token),
                                    generator_tool: tool.to_string(),
                                    version_constraint: None,
                                    generated_artifact: PathBuf::from(output),
                                    downstream_build_system: downstream.to_string(),
                                    bootstrap_script: Some(PathBuf::from(script_name)),
                                    description: format!(
                                        "{} executes '{}' on '{}' to generate '{}'",
                                        script_name, tool, clean_token, output
                                    ),
                                };

                                if !generations
                                    .iter()
                                    .any(|g| g.generated_artifact == gen.generated_artifact)
                                {
                                    generations.push(gen);
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    generations
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

    #[test]
    fn test_analyze_build_system_generation_autoconf() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("configure.ac"), "AC_INIT(my_proj, 1.0)\n").unwrap();
        fs::write(dir.path().join("autogen.sh"), "#!/bin/sh\nautoreconf -i\n").unwrap();

        let gens = analyze_build_system_generation(dir.path());
        assert_eq!(gens.len(), 1);
        assert_eq!(gens[0].input_declaration, PathBuf::from("configure.ac"));
        assert_eq!(gens[0].generator_tool, "autoreconf");
        assert_eq!(gens[0].generated_artifact, PathBuf::from("configure"));
        assert_eq!(gens[0].downstream_build_system, "configure");
        assert_eq!(gens[0].bootstrap_script, Some(PathBuf::from("autogen.sh")));
        assert!(!gens[0].is_artifact_present(dir.path()));

        // When configure is created
        fs::write(dir.path().join("configure"), "#!/bin/sh\n").unwrap();
        assert!(gens[0].is_artifact_present(dir.path()));

        // Also check bootstrap action created
        let actions = analyze_bootstrap(dir.path());
        assert!(actions
            .iter()
            .any(|a| a.action_type == "generate_build_system"
                && a.target_file == Path::new("configure")));
    }

    #[test]
    fn test_analyze_build_system_generation_custom_script() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("build.spec"), "spec content").unwrap();
        fs::write(
            dir.path().join("setup.sh"),
            "#!/bin/sh\ngenerator_a build.spec -o build.mk\n",
        )
        .unwrap();

        let gens = analyze_build_system_generation(dir.path());
        assert_eq!(gens.len(), 1);
        assert_eq!(gens[0].input_declaration, PathBuf::from("build.spec"));
        assert_eq!(gens[0].generator_tool, "generator_a");
        assert_eq!(gens[0].generated_artifact, PathBuf::from("build.mk"));
        assert_eq!(gens[0].downstream_build_system, "make");
        assert_eq!(gens[0].bootstrap_script, Some(PathBuf::from("setup.sh")));
    }
}

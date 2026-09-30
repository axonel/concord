use std::fs;
use std::path::{Path, PathBuf};
use toml::Value;
use concord_core::evidence::Evidence;
use concord_core::ir::{ProjectRequirement, RequirementKind, ToolScope};
use concord_core::VersionConstraint;

pub struct ToolVersionsDiscovery {
    pub requirements: Vec<ProjectRequirement>,
    pub evidence: Vec<Evidence>,
}

/// Classify a tool name and version string into the appropriate typed RequirementKind.
pub fn classify_tool(tool: &str, ver: &str) -> (String, RequirementKind) {
    let lower = tool.to_lowercase();
    let clean_tool = lower
        .strip_prefix("npm:")
        .or_else(|| lower.strip_prefix("github:"))
        .unwrap_or(&lower);

    // Normalize runtime aliases
    let base_name = match clean_tool {
        "nodejs" | "node" => "node",
        "python" | "python3" => "python",
        "golang" | "go" => "go",
        "rust" | "rustc" => "rust",
        "postgres" | "postgresql" => "postgresql",
        other => other,
    };

    let parsed_constraint = VersionConstraint::parse(ver);

    // 1. Runtimes
    if matches!(
        base_name,
        "node" | "python" | "ruby" | "java" | "rust" | "go" | "bun" | "php"
    ) {
        return (
            base_name.to_string(),
            RequirementKind::Runtime {
                name: base_name.to_string(),
                constraint: parsed_constraint,
            },
        );
    }

    // 2. Package managers
    if matches!(
        base_name,
        "pnpm" | "npm" | "yarn" | "cargo" | "uv" | "poetry"
    ) {
        return (
            base_name.to_string(),
            RequirementKind::PackageManager {
                name: base_name.to_string(),
                constraint: Some(parsed_constraint),
            },
        );
    }

    // 3. Services
    if matches!(base_name, "postgresql" | "mysql" | "redis" | "mongodb") {
        return (
            base_name.to_string(),
            RequirementKind::Service {
                name: base_name.to_string(),
                min_version: Some(ver.to_string()),
            },
        );
    }

    // 4. Code generators
    if base_name.contains("openapi-generator")
        || base_name.contains("oazapfts")
        || base_name.contains("protoc")
        || base_name.contains("sqlc")
    {
        return (
            tool.to_string(),
            RequirementKind::CodeGenerator {
                name: tool.to_string(),
                constraint: Some(parsed_constraint),
                scope: ToolScope::RequiredForTask,
            },
        );
    }

    // 5. Build tools
    if base_name.contains("binaryen")
        || base_name.contains("wasm-opt")
        || matches!(base_name, "make" | "cmake" | "ninja" | "gcc" | "clang")
    {
        return (
            tool.to_string(),
            RequirementKind::BuildTool {
                name: tool.to_string(),
                constraint: Some(parsed_constraint),
                scope: ToolScope::RequiredForBuild,
            },
        );
    }

    // 6. Developer / infra tools (default for unrecognized tools)
    let scope = ToolScope::RequiredForTask;
    (
        tool.to_string(),
        RequirementKind::DeveloperTool {
            name: tool.to_string(),
            constraint: Some(parsed_constraint),
            scope,
        },
    )
}

pub fn analyze_tool_versions(root: &Path) -> ToolVersionsDiscovery {
    let mut requirements = Vec::new();
    let mut evidence = Vec::new();

    // 1. .tool-versions
    let tool_versions_path = root.join(".tool-versions");
    if tool_versions_path.exists() {
        if let Ok(content) = fs::read_to_string(&tool_versions_path) {
            for (idx, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    let tool = parts[0];
                    let ver = parts[1];

                    let (req_name, kind) = classify_tool(tool, ver);

                    let ev = Evidence::from_repo_file(
                        PathBuf::from(".tool-versions"),
                        Some(idx + 1),
                        format!("Tool '{}' version {} declared in .tool-versions", tool, ver),
                    );

                    requirements.push(ProjectRequirement::new(req_name, kind, ev.clone()));
                    evidence.push(ev);
                }
            }
        }
    }

    // 2. .mise.toml / mise.toml / .mise/config.toml
    for mise_file in [".mise.toml", "mise.toml", ".mise/config.toml"] {
        let mise_path = root.join(mise_file);
        if mise_path.exists() {
            if let Ok(content) = fs::read_to_string(&mise_path) {
                if let Ok(toml) = content.parse::<Value>() {
                    if let Some(tools_table) = toml.get("tools").and_then(|t| t.as_table()) {
                        for (tool, val) in tools_table {
                            let ver_str = match val {
                                Value::String(s) => Some(s.clone()),
                                Value::Integer(i) => Some(i.to_string()),
                                Value::Float(f) => Some(f.to_string()),
                                _ => None,
                            };

                            if let Some(ver) = ver_str {
                                let (req_name, kind) = classify_tool(tool, &ver);

                                let ev = Evidence::from_repo_file(
                                    PathBuf::from(mise_file),
                                    None,
                                    format!(
                                        "Tool '{}' version {} declared in {}",
                                        tool, ver, mise_file
                                    ),
                                );

                                requirements.push(ProjectRequirement::new(
                                    req_name,
                                    kind,
                                    ev.clone(),
                                ));
                                evidence.push(ev);
                            }
                        }
                    }
                }
            }
            break;
        }
    }

    ToolVersionsDiscovery {
        requirements,
        evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_classify_mise_tools() {
        let (name, kind) = classify_tool("java", "21.0.2");
        assert_eq!(name, "java");
        assert_eq!(
            kind,
            RequirementKind::Runtime {
                name: "java".to_string(),
                constraint: VersionConstraint::Exact("21.0.2".to_string())
            }
        );

        let (name, kind) = classify_tool("pnpm", "11.24.0");
        assert_eq!(name, "pnpm");
        assert_eq!(
            kind,
            RequirementKind::PackageManager {
                name: "pnpm".to_string(),
                constraint: Some(VersionConstraint::Exact("11.24.0".to_string()))
            }
        );

        let (name, kind) = classify_tool("opentofu", "1.12.6");
        assert_eq!(name, "opentofu");
        assert!(matches!(kind, RequirementKind::DeveloperTool { .. }));

        let (_name, kind) = classify_tool("github:webassembly/binaryen", "version_124");
        assert!(matches!(kind, RequirementKind::BuildTool { .. }));

        let (_name, kind) = classify_tool("npm:oazapfts", "7.5.0");
        assert!(matches!(kind, RequirementKind::CodeGenerator { .. }));
    }

    #[test]
    fn test_parse_mise_exact_pins() {
        let dir = tempdir().unwrap();
        let mise_content = r#"
[tools]
node = "24.21.0"
pnpm = "11.24.0"
java = "21.0.2"
opentofu = "1.12.6"
"#;
        fs::write(dir.path().join("mise.toml"), mise_content).unwrap();

        let disc = analyze_tool_versions(dir.path());
        let java_req = disc.requirements.iter().find(|r| r.name == "java").unwrap();
        if let RequirementKind::Runtime { constraint, .. } = &java_req.kind {
            assert_eq!(constraint, &VersionConstraint::Exact("21.0.2".to_string()));
        // EXACT pin, NOT >=
        } else {
            panic!("Expected runtime requirement for java");
        }

        let pnpm_req = disc.requirements.iter().find(|r| r.name == "pnpm").unwrap();
        assert!(matches!(
            pnpm_req.kind,
            RequirementKind::PackageManager { .. }
        ));
    }
}

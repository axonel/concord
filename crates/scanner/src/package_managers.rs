use concord_core::evidence::{Evidence, EvidenceSource};
use concord_core::ir::PackageManagerObservation;
use concord_core::Confidence;
use std::path::{Path, PathBuf};
use std::process::Command;

struct PackageManagerProbe {
    name: &'static str,
    executable_candidates: &'static [&'static str],
    version_arg: &'static str,
}

const PM_PROBES: &[PackageManagerProbe] = &[
    PackageManagerProbe {
        name: "pnpm",
        executable_candidates: &["pnpm"],
        version_arg: "--version",
    },
    PackageManagerProbe {
        name: "npm",
        executable_candidates: &["npm"],
        version_arg: "--version",
    },
    PackageManagerProbe {
        name: "yarn",
        executable_candidates: &["yarn"],
        version_arg: "--version",
    },
    PackageManagerProbe {
        name: "bun",
        executable_candidates: &["bun"],
        version_arg: "--version",
    },
    PackageManagerProbe {
        name: "cargo",
        executable_candidates: &["cargo"],
        version_arg: "--version",
    },
    PackageManagerProbe {
        name: "uv",
        executable_candidates: &["uv"],
        version_arg: "--version",
    },
    PackageManagerProbe {
        name: "poetry",
        executable_candidates: &["poetry"],
        version_arg: "--version",
    },
];

fn resolve_in_path(binary: &str, path_entries: &[PathBuf]) -> Option<PathBuf> {
    for dir in path_entries {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn parse_first_semantic_version(output: &str) -> Option<String> {
    for word in output.split_whitespace() {
        let clean = word.trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
        let parts: Vec<&str> = clean.split('.').collect();
        if parts.len() >= 2
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        {
            return Some(clean.to_string());
        }
    }
    None
}

/// Scan package managers deterministically, respecting project context for version manager shims.
pub fn scan_package_managers(
    path_entries: &[PathBuf],
    project_context: Option<&Path>,
) -> Vec<PackageManagerObservation> {
    let mut observations = Vec::new();

    // 1. Direct mise inspection if available
    if let Some(proj_dir) = project_context {
        if let Ok(output) = Command::new("mise")
            .args(["ls", "--json"])
            .current_dir(proj_dir)
            .output()
        {
            if output.status.success() {
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                    if let Some(map) = json.as_object() {
                        for (tool_name, entries) in map {
                            if matches!(
                                tool_name.as_str(),
                                "pnpm" | "npm" | "yarn" | "bun" | "cargo" | "uv" | "poetry"
                            ) {
                                if let Some(arr) = entries.as_array() {
                                    for entry in arr {
                                        let installed = entry
                                            .get("installed")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false);
                                        if installed {
                                            let ver = entry
                                                .get("version")
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string());
                                            let install_path = entry
                                                .get("install_path")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let exe_path = if install_path.is_empty() {
                                                resolve_in_path(tool_name, path_entries)
                                                    .unwrap_or_else(|| PathBuf::from(tool_name))
                                            } else {
                                                let p1 =
                                                    PathBuf::from(install_path).join(tool_name);
                                                let p2 = PathBuf::from(install_path)
                                                    .join("bin")
                                                    .join(tool_name);
                                                if p1.is_file() {
                                                    p1
                                                } else if p2.is_file() {
                                                    p2
                                                } else {
                                                    resolve_in_path(tool_name, path_entries)
                                                        .unwrap_or(p1)
                                                }
                                            };

                                            let evidence = Evidence::new(
                                                EvidenceSource::ExecutableInspection {
                                                    path: exe_path.clone(),
                                                    version_string: ver.clone().unwrap_or_default(),
                                                    exit_code: 0,
                                                },
                                                Confidence::Confirmed,
                                                format!(
                                                    "Package manager '{}' discovered via mise with version {}",
                                                    tool_name,
                                                    ver.as_deref().unwrap_or("unknown")
                                                ),
                                            );

                                            observations.push(PackageManagerObservation {
                                                name: tool_name.clone(),
                                                version: ver,
                                                executable_path: exe_path,
                                                evidence,
                                            });
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. PATH resolution and execution probe
    for probe in PM_PROBES {
        if observations.iter().any(|o| o.name == probe.name) {
            continue;
        }

        for candidate_name in probe.executable_candidates {
            if let Some(executable_path) = resolve_in_path(candidate_name, path_entries) {
                let mut cmd = Command::new(&executable_path);
                cmd.arg(probe.version_arg);
                if let Some(dir) = project_context {
                    cmd.current_dir(dir);
                }

                if let Ok(out) = cmd.output() {
                    if out.status.success() {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        let combined = if stdout.trim().is_empty() {
                            stderr.to_string()
                        } else {
                            stdout.to_string()
                        };

                        let ver = parse_first_semantic_version(&combined);

                        let evidence = Evidence::new(
                            EvidenceSource::ExecutableInspection {
                                path: executable_path.clone(),
                                version_string: combined.trim().to_string(),
                                exit_code: 0,
                            },
                            Confidence::Confirmed,
                            format!(
                                "Package manager '{}' discovered at {} with version {}",
                                probe.name,
                                executable_path.display(),
                                ver.as_deref().unwrap_or("unknown")
                            ),
                        );

                        observations.push(PackageManagerObservation {
                            name: probe.name.to_string(),
                            version: ver,
                            executable_path,
                            evidence,
                        });
                        break;
                    }
                }
            }
        }
    }

    observations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pm_version() {
        assert_eq!(
            parse_first_semantic_version("pnpm 11.24.0\n").as_deref(),
            Some("11.24.0")
        );
        assert_eq!(
            parse_first_semantic_version("10.5.2\n").as_deref(),
            Some("10.5.2")
        );
        assert_eq!(
            parse_first_semantic_version("cargo 1.80.0 (051478957 2024-07-21)").as_deref(),
            Some("1.80.0")
        );
    }
}

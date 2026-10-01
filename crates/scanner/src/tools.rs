use concord_core::evidence::{Evidence, EvidenceSource};
use concord_core::ir::{ToolKind, ToolObservation};
use concord_core::Confidence;
use std::path::{Path, PathBuf};
use std::process::Command;

fn resolve_in_path(binary: &str, path_entries: &[PathBuf]) -> Option<PathBuf> {
    for dir in path_entries {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub use concord_core::version::parse_first_semantic_version;

/// Classify known tool binary names into their appropriate ToolKind.
pub fn classify_tool_kind(name: &str) -> ToolKind {
    ToolKind::classify(name)
}

/// Probe a specific tool by binary name in search directories, inspecting its version if present.
pub fn probe_tool(
    name: &str,
    search_dirs: &[PathBuf],
    project_context: Option<&Path>,
) -> Option<ToolObservation> {
    let executable_path = resolve_in_path(name, search_dirs)?;
    let mut cmd = Command::new(&executable_path);
    cmd.arg("--version");
    if let Some(dir) = project_context {
        cmd.current_dir(dir);
    }

    let mut out = cmd.output();
    if out.as_ref().map(|o| !o.status.success()).unwrap_or(true) {
        // Fallback for tools expecting -version (e.g. swig)
        let mut alt_cmd = Command::new(&executable_path);
        alt_cmd.arg("-version");
        if let Some(dir) = project_context {
            alt_cmd.current_dir(dir);
        }
        if let Ok(alt_out) = alt_cmd.output() {
            if alt_out.status.success() {
                out = Ok(alt_out);
            }
        }
    }
    if out.as_ref().map(|o| !o.status.success()).unwrap_or(true) {
        let mut alt_cmd = Command::new(&executable_path);
        alt_cmd.arg("-v");
        if let Some(dir) = project_context {
            alt_cmd.current_dir(dir);
        }
        if let Ok(alt_out) = alt_cmd.output() {
            if alt_out.status.success() {
                out = Ok(alt_out);
            }
        }
    }

    let (ver, ver_str) = match out {
        Ok(ref o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stderr = String::from_utf8_lossy(&o.stderr);
            let combined = if stdout.trim().is_empty() {
                stderr.to_string()
            } else {
                stdout.to_string()
            };
            let ver = parse_first_semantic_version(&combined);
            (ver, combined.trim().to_string())
        }
        _ => (None, "present".to_string()),
    };

    let kind = classify_tool_kind(name);
    let evidence = Evidence::new(
        EvidenceSource::ExecutableInspection {
            path: executable_path.clone(),
            version_string: ver_str,
            exit_code: 0,
        },
        Confidence::Confirmed,
        format!(
            "Tool '{}' ({}) discovered at {}",
            name,
            kind,
            executable_path.display()
        ),
    );

    Some(ToolObservation {
        name: name.to_string(),
        kind,
        version: ver,
        executable_path,
        evidence,
    })
}

/// Scan developer and build tools on the host system.
pub fn scan_tools(
    path_entries: &[PathBuf],
    project_context: Option<&Path>,
) -> Vec<ToolObservation> {
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
                            // Skip runtimes and package managers handled elsewhere
                            if matches!(
                                tool_name.as_str(),
                                "node"
                                    | "python"
                                    | "java"
                                    | "rust"
                                    | "go"
                                    | "bun"
                                    | "pnpm"
                                    | "npm"
                                    | "yarn"
                                    | "cargo"
                                    | "uv"
                                    | "poetry"
                            ) {
                                continue;
                            }

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
                                            let p1 = PathBuf::from(install_path).join(tool_name);
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

                                        let kind = classify_tool_kind(tool_name);
                                        let evidence = Evidence::new(
                                            EvidenceSource::ExecutableInspection {
                                                path: exe_path.clone(),
                                                version_string: ver.clone().unwrap_or_default(),
                                                exit_code: 0,
                                            },
                                            Confidence::Confirmed,
                                            format!(
                                                "Tool '{}' ({}) discovered via mise with version {}",
                                                tool_name,
                                                kind,
                                                ver.as_deref().unwrap_or("unknown")
                                            ),
                                        );

                                        observations.push(ToolObservation {
                                            name: tool_name.clone(),
                                            kind,
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

    // 2. PATH resolution for common developer/build tools and code generators
    let common_tools = &[
        "make",
        "cmake",
        "ninja",
        "meson",
        "pkg-config",
        "pkgconf",
        "gcc",
        "clang",
        "cc",
        "g++",
        "clang++",
        "c++",
        "nvcc",
        "gfortran",
        "flang",
        "opentofu",
        "terraform",
        "terragrunt",
        "wasm-opt",
        "extism",
        // Standard build generators and core tools
        "autoreconf",
        "autoconf",
        "automake",
        "libtool",
        "m4",
        "cat",
        "more",
        "sed",
        "awk",
        "tar",
        "gzip",
        "git",
        "doxygen",
        // Common code generators
        "bison",
        "yacc",
        "byacc",
        "flex",
        "lex",
        "gperf",
        "ragel",
        "swig",
        "protoc",
        "flatc",
        "capnp",
        "thrift",
        "wayland-scanner",
        "glib-compile-resources",
        "glib-mkenums",
        "glib-genmarshal",
        "bindgen",
        "cbindgen",
        "rpcgen",
    ];

    let mut search_dirs = path_entries.to_vec();
    // Standard toolkit directories (e.g. CUDA toolkit) that might not be in minimal PATH
    for extra in ["/usr/local/cuda/bin", "/opt/cuda/bin"] {
        let p = PathBuf::from(extra);
        if p.is_dir() && !search_dirs.contains(&p) {
            search_dirs.push(p);
        }
    }

    for tool_name in common_tools {
        if observations.iter().any(|o| o.name == *tool_name) {
            continue;
        }

        if let Some(obs) = probe_tool(tool_name, &search_dirs, project_context) {
            observations.push(obs);
        }
    }

    observations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_tool_kind() {
        assert_eq!(classify_tool_kind("make"), ToolKind::BuildTool);
        assert_eq!(classify_tool_kind("cmake"), ToolKind::BuildTool);
        assert_eq!(classify_tool_kind("wasm-opt"), ToolKind::BuildTool);
        assert_eq!(classify_tool_kind("binaryen"), ToolKind::BuildTool);
        assert_eq!(classify_tool_kind("nvcc"), ToolKind::BuildTool);
        assert_eq!(classify_tool_kind("gfortran"), ToolKind::BuildTool);
        assert_eq!(classify_tool_kind("flang"), ToolKind::BuildTool);
        assert_eq!(classify_tool_kind("c++"), ToolKind::BuildTool);
        assert_eq!(classify_tool_kind("npm:oazapfts"), ToolKind::CodeGenerator);
        assert_eq!(classify_tool_kind("protoc"), ToolKind::CodeGenerator);
        assert_eq!(classify_tool_kind("bison"), ToolKind::CodeGenerator);
        assert_eq!(classify_tool_kind("byacc"), ToolKind::CodeGenerator);
        assert_eq!(classify_tool_kind("flex"), ToolKind::CodeGenerator);
        assert_eq!(classify_tool_kind("swig"), ToolKind::CodeGenerator);
        assert_eq!(classify_tool_kind("bindgen"), ToolKind::CodeGenerator);
        assert_eq!(
            classify_tool_kind("wayland-scanner"),
            ToolKind::CodeGenerator
        );
        assert_eq!(classify_tool_kind("opentofu"), ToolKind::DeveloperTool);
        assert_eq!(classify_tool_kind("terragrunt"), ToolKind::DeveloperTool);
    }

    #[test]
    fn test_parse_first_semantic_version_nvcc() {
        let nvcc_output = "nvcc: NVIDIA (R) Cuda compiler driver\nCopyright (c) 2005-2023 NVIDIA Corporation\nBuilt on Wed_Nov_22_10:17:15_PST_2023\nCuda compilation tools, release 12.3, V12.3.107\nBuild cuda_12.3.r12.3/compiler.33567101_0\n";
        assert_eq!(
            parse_first_semantic_version(nvcc_output),
            Some("12.3.107".to_string())
        );
    }

    #[test]
    fn test_parse_first_semantic_version_standard() {
        let gcc_output = "gcc (Ubuntu 11.4.0-1ubuntu1~22.04) 11.4.0";
        assert_eq!(
            parse_first_semantic_version(gcc_output),
            Some("11.4.0".to_string())
        );

        let make_output = "GNU Make 4.3";
        assert_eq!(
            parse_first_semantic_version(make_output),
            Some("4.3".to_string())
        );
    }
}

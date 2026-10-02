use concord_core::evidence::{Evidence, EvidenceSource};
use concord_core::ir::{ProjectRequirement, RequirementKind, ToolKind, ToolScope};
use concord_core::Confidence;
use concord_core::VersionConstraint;
use std::fs;
use std::path::{Path, PathBuf};

pub struct MesonDiscovery {
    pub is_meson: bool,
    pub languages: Vec<String>,
    pub requirements: Vec<ProjectRequirement>,
    pub evidence: Vec<Evidence>,
}

fn strip_quotes(s: &str) -> &str {
    s.trim().trim_matches(|c| c == '\'' || c == '"')
}

fn parse_meson_version_spec(line: &str) -> Option<String> {
    if let Some(idx) = line.find("meson_version:") {
        let after = &line[idx + "meson_version:".len()..];
        let val = after.split(',').next()?.trim();
        let stripped = strip_quotes(val);
        if !stripped.is_empty() {
            return Some(stripped.to_string());
        }
    }
    None
}

fn parse_c_std(content: &str) -> Option<String> {
    for line in content.lines() {
        if line.contains("c_std=") {
            if let Some(idx) = line.find("c_std=") {
                let after = &line[idx + "c_std=".len()..];
                let val = after
                    .trim_matches(|c| c == '\'' || c == '"' || c == ',' || c == ']' || c == ' ');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

fn extract_quoted_string(s: &str) -> Option<&str> {
    let trimmed = s.trim();
    if ((trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('"') && trimmed.ends_with('"')))
        && trimmed.len() >= 2
    {
        let inner = &trimmed[1..trimmed.len() - 1];
        if !inner.is_empty() && !inner.contains(' ') && !inner.contains('(') && !inner.contains(')')
        {
            return Some(inner);
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct MesonOption {
    pub name: String,
    pub opt_type: String,
    pub default_val: Option<String>,
}

fn parse_meson_options(root: &Path) -> std::collections::HashMap<String, MesonOption> {
    let mut options = std::collections::HashMap::new();
    let opt_files = [root.join("meson_options.txt"), root.join("meson.options")];
    for opt_path in &opt_files {
        if let Ok(content) = fs::read_to_string(opt_path) {
            for line in content.lines() {
                let code = match line.split_once('#') {
                    Some((b, _)) => b.trim(),
                    None => line.trim(),
                };
                if code.starts_with("option(") {
                    if let Some(inner) = code
                        .strip_prefix("option(")
                        .and_then(|s| s.strip_suffix(')'))
                    {
                        let parts: Vec<&str> = inner.split(',').collect();
                        if let Some(name_raw) = parts.first() {
                            if let Some(name) = extract_quoted_string(name_raw) {
                                let mut opt_type = "string".to_string();
                                let mut default_val = None;
                                for part in &parts[1..] {
                                    let part_trimmed = part.trim();
                                    if let Some((k, v)) = part_trimmed.split_once(':') {
                                        let k = k.trim();
                                        let v = v.trim();
                                        if k == "type" {
                                            if let Some(t) = extract_quoted_string(v) {
                                                opt_type = t.to_string();
                                            }
                                        } else if k == "value" {
                                            default_val = Some(strip_quotes(v).to_string());
                                        }
                                    }
                                }
                                options.insert(
                                    name.to_string(),
                                    MesonOption {
                                        name: name.to_string(),
                                        opt_type,
                                        default_val,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    options
}

fn parse_modules(content: &str) -> Vec<String> {
    let mut modules = Vec::new();
    for line in content.lines() {
        let code = match line.split_once('#') {
            Some((before, _)) => before.trim(),
            None => line.trim(),
        };
        // Only parse if line relates to python modules or modules: keyword argument
        let is_python_context = code.contains("python") || code.contains("py3");
        let is_modules_kw = code.contains("modules:") || code.contains("modules :");
        if !is_python_context && !is_modules_kw {
            continue;
        }

        if let Some(idx) = code.find("modules") {
            let after = &code[idx + "modules".len()..];
            let trimmed_after = after.trim_start();
            if trimmed_after.starts_with(':') || trimmed_after.starts_with('=') {
                if let Some(start_bracket) = code.find('[') {
                    if let Some(end_bracket) = code[start_bracket..].find(']') {
                        let list_str = &code[start_bracket + 1..start_bracket + end_bracket];
                        for item in list_str.split(',') {
                            let item_clean = match item.split_once('#') {
                                Some((before, _)) => before.trim(),
                                None => item.trim(),
                            };
                            if let Some(clean) = extract_quoted_string(item_clean) {
                                modules.push(clean.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    modules.sort();
    modules.dedup();
    modules
}

fn parse_required_arg_with_options(
    text: &str,
    options: &std::collections::HashMap<String, MesonOption>,
) -> (Option<bool>, Option<String>) {
    let idx = match text.find("required:").or_else(|| text.find("required :")) {
        Some(i) => i,
        None => return (None, None),
    };
    let after = text[idx..]
        .split_once(':')
        .map(|x| x.1.trim())
        .unwrap_or("");

    if after.starts_with("false") || after.starts_with("'false'") || after.starts_with("\"false\"")
    {
        return (Some(false), None);
    }
    if after.starts_with("true") || after.starts_with("'true'") || after.starts_with("\"true\"") {
        return (Some(true), None);
    }

    if let Some(opt_idx) = after.find("get_option(") {
        let opt_inner = &after[opt_idx + "get_option(".len()..];
        if let Some(first) = opt_inner.split(')').next() {
            if let Some(opt_name) = extract_quoted_string(first) {
                if let Some(opt) = options.get(opt_name) {
                    if let Some(ref val) = opt.default_val {
                        let val_lower = val.to_lowercase();
                        if val_lower == "false" || val_lower == "disabled" {
                            return (
                                Some(false),
                                Some(format!("option '{}' defaults to false/disabled", opt_name)),
                            );
                        } else if val_lower == "true" || val_lower == "enabled" {
                            return (
                                Some(true),
                                Some(format!("option '{}' defaults to true/enabled", opt_name)),
                            );
                        } else if val_lower == "auto" {
                            return (
                                Some(false),
                                Some(format!("option '{}' defaults to auto (optional)", opt_name)),
                            );
                        }
                    } else if opt.opt_type == "feature" {
                        return (
                            Some(false),
                            Some(format!(
                                "feature option '{}' defaults to auto (optional)",
                                opt_name
                            )),
                        );
                    }
                } else {
                    return (
                        None,
                        Some(format!("get_option('{}') default is unknown", opt_name)),
                    );
                }
            }
        }
    }

    (None, None)
}

#[derive(Debug, Clone)]
struct MesonConditionScope {
    platform: Option<String>,
    arch: Option<String>,
}

fn eval_meson_condition(code: &str) -> (Option<String>, Option<String>) {
    let mut platform = None;
    if code.contains("host_machine.system() == 'windows'")
        || code.contains("host_machine.system() == \"windows\"")
        || code.contains("is_windows")
    {
        platform = Some("windows".to_string());
    } else if code.contains("host_machine.system() == 'darwin'")
        || code.contains("host_machine.system() == \"darwin\"")
        || code.contains("is_darwin")
    {
        platform = Some("darwin".to_string());
    } else if code.contains("host_machine.system() == 'linux'")
        || code.contains("host_machine.system() == \"linux\"")
        || code.contains("is_linux")
    {
        platform = Some("linux".to_string());
    }

    let mut arch = None;
    let is_arch_cond = code.contains("cpu_family()")
        || code.contains("target_machine.cpu_family()")
        || code.contains("host_machine.cpu_family()")
        || code.contains("build_machine.cpu_family()")
        || code.contains("cpu()")
        || code.contains("target_machine.cpu()")
        || code.contains("host_machine.cpu()");

    if is_arch_cond {
        // If negated (!= or not), do not declare positive architecture guard
        if code.contains("!=") || code.starts_with("not ") || code.contains(" not ") {
            return (platform, None);
        }

        if code.contains("'x86_64'")
            || code.contains("\"x86_64\"")
            || code.contains("'amd64'")
            || code.contains("\"amd64\"")
        {
            arch = Some("x86_64".to_string());
        } else if code.contains("'aarch64'")
            || code.contains("\"aarch64\"")
            || code.contains("'arm64'")
            || code.contains("\"arm64\"")
        {
            arch = Some("aarch64".to_string());
        } else if code.contains("'arm'")
            || code.contains("\"arm\"")
            || code.contains("'armv7'")
            || code.contains("\"armv7\"")
        {
            arch = Some("armv7".to_string());
        } else if code.contains("'riscv64'") || code.contains("\"riscv64\"") {
            arch = Some("riscv64".to_string());
        } else if code.contains("'ppc64'") || code.contains("\"ppc64\"") {
            arch = Some("ppc64le".to_string());
        } else if code.contains("'x86'") || code.contains("\"x86\"") || code.contains("'i386'") {
            arch = Some("x86".to_string());
        } else if code.contains("'s390x'") || code.contains("\"s390x\"") {
            arch = Some("s390x".to_string());
        } else {
            // It tests cpu_family() / cpu(), but the operand is dynamic / unresolved (variable, call, etc.)
            // Preserving uncertainty via dynamic_arch identifier
            arch = Some("dynamic_arch".to_string());
        }
    }

    (platform, arch)
}

struct MesonCall {
    pub name: String,
    pub call_text: String,
    pub line_no: usize,
    pub platform: Option<String>,
    pub arch: Option<String>,
}

fn find_meson_calls(content: &str, func_name: &str) -> Vec<MesonCall> {
    let mut calls = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut condition_stack: Vec<MesonConditionScope> = Vec::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let code = match line.split_once('#') {
            Some((before, _)) => before.trim(),
            None => line.trim(),
        };
        if code.is_empty() {
            continue;
        }

        if code.starts_with("if ") || code.contains(" if ") {
            let (plat, arch) = eval_meson_condition(code);
            condition_stack.push(MesonConditionScope {
                platform: plat,
                arch,
            });
        } else if code.starts_with("elif ") || code.contains(" elif ") {
            let (plat, arch) = eval_meson_condition(code);
            if let Some(scope) = condition_stack.last_mut() {
                scope.platform = plat;
                scope.arch = arch;
            } else {
                condition_stack.push(MesonConditionScope {
                    platform: plat,
                    arch,
                });
            }
        } else if code == "else" || code.starts_with("else ") || code.starts_with("else:") {
            if let Some(scope) = condition_stack.last_mut() {
                scope.platform = None;
                scope.arch = None;
            }
        } else if code == "endif" || code.starts_with("endif ") {
            condition_stack.pop();
        }

        let current_platform = condition_stack
            .iter()
            .rev()
            .find_map(|s| s.platform.clone());
        let current_arch = condition_stack.iter().rev().find_map(|s| s.arch.clone());

        let pattern = format!("{}(", func_name);
        let pattern_space = format!("{} (", func_name);

        let mut start_pos = None;
        if let Some(pos) = code.find(&pattern) {
            start_pos = Some(pos + pattern.len());
        } else if let Some(pos) = code.find(&pattern_space) {
            start_pos = Some(pos + pattern_space.len());
        }

        if let Some(pos) = start_pos {
            let before_match = &code[..pos.saturating_sub(pattern.len()).min(pos)];
            if (func_name == "dependency" && before_match.ends_with("declare_"))
                || (func_name == "find_program" && before_match.ends_with("override_"))
            {
                continue;
            }

            let mut call_text = String::new();
            let mut found_close = false;
            let mut paren_depth = 1;

            let rest_of_line = &code[pos..];
            for c in rest_of_line.chars() {
                if c == '(' {
                    paren_depth += 1;
                } else if c == ')' {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        found_close = true;
                        break;
                    }
                }
                call_text.push(c);
            }

            if !found_close {
                let mut next_line_idx = line_idx + 1;
                while next_line_idx < lines.len() && !found_close {
                    let next_code = match lines[next_line_idx].split_once('#') {
                        Some((before, _)) => before.trim(),
                        None => lines[next_line_idx].trim(),
                    };
                    call_text.push(' ');
                    for c in next_code.chars() {
                        if c == '(' {
                            paren_depth += 1;
                        } else if c == ')' {
                            paren_depth -= 1;
                            if paren_depth == 0 {
                                found_close = true;
                                break;
                            }
                        }
                        call_text.push(c);
                    }
                    next_line_idx += 1;
                }
            }

            if let Some(first_arg) = call_text.split(',').next() {
                if let Some(name) = extract_quoted_string(first_arg) {
                    calls.push(MesonCall {
                        name: name.to_string(),
                        call_text,
                        line_no: line_idx + 1,
                        platform: current_platform.clone(),
                        arch: current_arch.clone(),
                    });
                }
            }
        }
    }

    calls
}

fn parse_meson_find_program(
    call: &MesonCall,
    rel_path: &Path,
    options: &std::collections::HashMap<String, MesonOption>,
) -> Option<ProjectRequirement> {
    let mut candidate_names: Vec<String> = Vec::new();
    let (is_req_opt, note) = parse_required_arg_with_options(&call.call_text, options);
    let scope = match is_req_opt {
        Some(true) => ToolScope::RequiredForBuild,
        Some(false) => ToolScope::Optional,
        None => {
            if call.call_text.contains("get_option") {
                ToolScope::Optional
            } else {
                ToolScope::RequiredForBuild
            }
        }
    };

    let mut ver_constraint = None;
    if let Some(idx) = call.call_text.find("version:") {
        let after = &call.call_text[idx + "version:".len()..];
        if let Some(first) = after.split(',').next() {
            let val = strip_quotes(first.trim());
            if !val.is_empty() {
                ver_constraint = Some(VersionConstraint::parse(val));
            }
        }
    }

    for arg in call.call_text.split(',') {
        let arg_trimmed = arg.trim();
        if arg_trimmed.contains(':') {
            continue;
        }
        if let Some(name) = extract_quoted_string(arg_trimmed) {
            if !name.contains('/')
                && !name.ends_with(".sh")
                && !name.ends_with(".py")
                && !name.ends_with(".pl")
                && !candidate_names.contains(&name.to_string())
            {
                candidate_names.push(name.to_string());
            }
        }
    }

    if candidate_names.is_empty() {
        return None;
    }

    if candidate_names.len() == 1 {
        let prog_name = &candidate_names[0];
        let tool_kind = ToolKind::classify(prog_name);
        let detail = if let Some(n) = note {
            format!(
                "Meson find_program('{}') (scope: {:?}, note: {})",
                prog_name, scope, n
            )
        } else {
            format!(
                "Meson find_program('{}') declared (scope: {:?})",
                prog_name, scope
            )
        };
        let ev = Evidence::new(
            EvidenceSource::BuildConfiguration {
                path: rel_path.to_path_buf(),
                line: Some(call.line_no),
                detail: Some(detail),
            },
            Confidence::High,
            format!(
                "{} '{}' declared in Meson build configuration ({:?})",
                tool_kind, prog_name, scope
            ),
        );
        let req_kind = match tool_kind {
            ToolKind::CodeGenerator => RequirementKind::CodeGenerator {
                name: prog_name.to_lowercase(),
                constraint: ver_constraint,
                scope,
            },
            _ => RequirementKind::BuildTool {
                name: prog_name.to_lowercase(),
                constraint: ver_constraint,
                scope,
            },
        };
        let mut req = ProjectRequirement::new(prog_name.to_lowercase(), req_kind, ev);
        if let Some(ref p) = call.platform {
            req = req.with_platform(p.clone());
        }
        if let Some(ref a) = call.arch {
            req = req.with_arch(a.clone());
        }
        Some(req)
    } else {
        let cap_name = format!("{}_executable", candidate_names[0].to_lowercase());
        let ev = Evidence::new(
            EvidenceSource::BuildConfiguration {
                path: rel_path.to_path_buf(),
                line: Some(call.line_no),
                detail: Some(format!(
                    "Meson find_program alternative candidates: {} (scope: {:?})",
                    candidate_names.join(", "),
                    scope
                )),
            },
            Confidence::High,
            format!(
                "Alternative tools [{}] declared in Meson find_program ({:?})",
                candidate_names.join(", "),
                scope
            ),
        );
        let alternatives: Vec<ProjectRequirement> = candidate_names
            .iter()
            .map(|prog_name| {
                let tool_kind = ToolKind::classify(prog_name);
                let alt_ev = Evidence::new(
                    EvidenceSource::BuildConfiguration {
                        path: rel_path.to_path_buf(),
                        line: Some(call.line_no),
                        detail: Some(format!(
                            "Alternative {} candidate '{}'",
                            tool_kind, prog_name
                        )),
                    },
                    Confidence::High,
                    format!("Alternative candidate '{}'", prog_name),
                );
                let req_kind = match tool_kind {
                    ToolKind::CodeGenerator => RequirementKind::CodeGenerator {
                        name: prog_name.to_lowercase(),
                        constraint: ver_constraint.clone(),
                        scope,
                    },
                    _ => RequirementKind::BuildTool {
                        name: prog_name.to_lowercase(),
                        constraint: ver_constraint.clone(),
                        scope,
                    },
                };
                let mut alt_req =
                    ProjectRequirement::new(prog_name.to_lowercase(), req_kind, alt_ev);
                if let Some(ref p) = call.platform {
                    alt_req = alt_req.with_platform(p.clone());
                }
                if let Some(ref a) = call.arch {
                    alt_req = alt_req.with_arch(a.clone());
                }
                alt_req
            })
            .collect();

        let mut req = ProjectRequirement::new(
            cap_name.clone(),
            RequirementKind::AnyOf {
                capability: cap_name,
                alternatives,
                scope,
            },
            ev,
        );
        if let Some(ref p) = call.platform {
            req = req.with_platform(p.clone());
        }
        if let Some(ref a) = call.arch {
            req = req.with_arch(a.clone());
        }
        Some(req)
    }
}

/// Analyze Meson project configurations (meson.build and included subdirs).
pub fn analyze_meson(root: &Path) -> MesonDiscovery {
    let mut is_meson = false;
    let mut languages = Vec::new();
    let mut requirements = Vec::new();
    let mut evidence = Vec::new();

    let root_meson = root.join("meson.build");
    if !root_meson.exists() {
        return MesonDiscovery {
            is_meson,
            languages,
            requirements,
            evidence,
        };
    }

    is_meson = true;

    // Collect all meson.build files in root and immediate build/config subdirs
    let mut build_files = vec![PathBuf::from("meson.build")];
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let sub_meson = path.join("meson.build");
                if sub_meson.is_file() {
                    if let Ok(rel) = sub_meson.strip_prefix(root) {
                        build_files.push(rel.to_path_buf());
                    }
                }
            }
        }
    }

    // 1. Analyze root meson.build
    if let Ok(content) = fs::read_to_string(&root_meson) {
        let root_ev = Evidence::new(
            EvidenceSource::BuildConfiguration {
                path: PathBuf::from("meson.build"),
                line: None,
                detail: Some("Meson project definition found".to_string()),
            },
            Confidence::Confirmed,
            "Meson build configuration detected via meson.build",
        );
        evidence.push(root_ev);

        let mut meson_ver_constraint = None;
        let mut has_c = false;
        let mut has_cpp = false;

        // Extract project(...)
        for (line_no, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("project(") || trimmed.starts_with("project (") {
                // Parse language arguments
                if trimmed.contains("'c'") || trimmed.contains("\"c\"") {
                    has_c = true;
                }
                if trimmed.contains("'cpp'") || trimmed.contains("\"cpp\"") {
                    has_cpp = true;
                }
            }

            if trimmed.contains("meson_version") {
                if let Some(spec) = parse_meson_version_spec(trimmed) {
                    meson_ver_constraint = Some(VersionConstraint::parse(&spec));
                    let ev = Evidence::new(
                        EvidenceSource::BuildConfiguration {
                            path: PathBuf::from("meson.build"),
                            line: Some(line_no + 1),
                            detail: Some(format!("meson_version declared: {}", spec)),
                        },
                        Confidence::High,
                        format!("Meson build system minimum version requirement: {}", spec),
                    );
                    requirements.push(ProjectRequirement::new(
                        "meson",
                        RequirementKind::BuildTool {
                            name: "meson".to_string(),
                            constraint: meson_ver_constraint.clone(),
                            scope: ToolScope::RequiredForBuild,
                        },
                        ev.clone(),
                    ));
                    evidence.push(ev);
                }
            }
        }

        // If meson_version was not explicitly set, still emit a general build tool requirement for meson
        if meson_ver_constraint.is_none() {
            let ev = Evidence::new(
                EvidenceSource::BuildConfiguration {
                    path: PathBuf::from("meson.build"),
                    line: None,
                    detail: Some("Meson build system required".to_string()),
                },
                Confidence::High,
                "Meson build tool required to configure project",
            );
            requirements.push(ProjectRequirement::new(
                "meson",
                RequirementKind::BuildTool {
                    name: "meson".to_string(),
                    constraint: None,
                    scope: ToolScope::RequiredForBuild,
                },
                ev.clone(),
            ));
            evidence.push(ev);
        }

        // Ninja backend requirement (default for Meson)
        let ninja_ev = Evidence::new(
            EvidenceSource::InferredRequirement {
                detail: "Meson default build backend is Ninja".to_string(),
            },
            Confidence::Medium,
            "Ninja build tool required as default Meson backend",
        );
        requirements.push(ProjectRequirement::new(
            "ninja",
            RequirementKind::BuildTool {
                name: "ninja".to_string(),
                constraint: None,
                scope: ToolScope::RequiredForBuild,
            },
            ninja_ev.clone(),
        ));
        evidence.push(ninja_ev);

        // Language & standard compiler requirements
        let c_std = parse_c_std(&content);
        if has_c {
            languages.push("c".to_string());
            let ev = Evidence::new(
                EvidenceSource::BuildConfiguration {
                    path: PathBuf::from("meson.build"),
                    line: None,
                    detail: Some(format!(
                        "C compiler required{}",
                        c_std
                            .as_deref()
                            .map(|s| format!(" ({})", s))
                            .unwrap_or_default()
                    )),
                },
                Confidence::High,
                format!(
                    "C compiler required for build{}",
                    c_std
                        .as_deref()
                        .map(|s| format!(" with standard {}", s))
                        .unwrap_or_default()
                ),
            );
            requirements.push(ProjectRequirement::new(
                "c",
                RequirementKind::Compiler {
                    language: "c".to_string(),
                    min_standard: c_std,
                    constraint: None,
                },
                ev.clone(),
            ));
            evidence.push(ev);
        }

        if has_cpp {
            languages.push("cpp".to_string());
            let ev = Evidence::new(
                EvidenceSource::BuildConfiguration {
                    path: PathBuf::from("meson.build"),
                    line: None,
                    detail: Some("C++ compiler required".to_string()),
                },
                Confidence::High,
                "C++ compiler required for build",
            );
            requirements.push(ProjectRequirement::new(
                "cpp",
                RequirementKind::Compiler {
                    language: "cpp".to_string(),
                    min_standard: None,
                    constraint: None,
                },
                ev.clone(),
            ));
            evidence.push(ev);
        }
    }

    let meson_options = parse_meson_options(root);

    // 2. Scan all discovered meson.build files for dependencies, python modules, and system libraries
    let mut has_python = false;
    let mut python_evidence = None;
    let mut seen_modules: std::collections::BTreeMap<String, (PathBuf, usize)> =
        std::collections::BTreeMap::new();
    type MesonLibKey = (String, Option<String>, Option<String>);
    type MesonLibEntry = (ToolScope, Evidence);
    let mut seen_libs: std::collections::BTreeMap<MesonLibKey, MesonLibEntry> =
        std::collections::BTreeMap::new();

    for rel_path in &build_files {
        let full_path = root.join(rel_path);
        if let Ok(content) = fs::read_to_string(&full_path) {
            let file_modules = parse_modules(&content);
            let file_has_python = content.contains("find_installation('python")
                || content.contains("find_installation(\"python")
                || content.contains("import('python')")
                || content.contains("import(\"python\")")
                || !file_modules.is_empty();

            if file_has_python {
                has_python = true;
                if python_evidence.is_none() {
                    python_evidence = Some(Evidence::new(
                        EvidenceSource::BuildConfiguration {
                            path: rel_path.clone(),
                            line: None,
                            detail: Some("Python3 required by build configuration".to_string()),
                        },
                        Confidence::High,
                        "Python 3 runtime required by build tools",
                    ));
                }

                for mod_name in file_modules {
                    seen_modules
                        .entry(mod_name)
                        .or_insert_with(|| (rel_path.clone(), 1));
                }
            }

            // Extract dependency(...) and find_library(...) calls
            let dep_calls = find_meson_calls(&content, "dependency");
            let find_lib_calls = find_meson_calls(&content, "find_library");

            for call in dep_calls.into_iter().chain(find_lib_calls) {
                let (is_req_opt, note) =
                    parse_required_arg_with_options(&call.call_text, &meson_options);
                let scope = match is_req_opt {
                    Some(true) => ToolScope::RequiredForBuild,
                    Some(false) => ToolScope::Optional,
                    None => {
                        if call.call_text.contains("get_option") {
                            // Documented uncertainty: unknown option default, treat as optional
                            ToolScope::Optional
                        } else {
                            ToolScope::RequiredForBuild
                        }
                    }
                };

                let detail = if let Some(n) = note {
                    format!(
                        "System library '{}' (scope: {:?}, note: {})",
                        call.name, scope, n
                    )
                } else {
                    format!("System library '{}' (scope: {:?})", call.name, scope)
                };

                let ev = Evidence::new(
                    EvidenceSource::BuildConfiguration {
                        path: rel_path.clone(),
                        line: Some(call.line_no),
                        detail: Some(detail),
                    },
                    Confidence::High,
                    format!(
                        "System library '{}' declared in build configuration ({:?})",
                        call.name, scope
                    ),
                );

                let lib_key = (call.name, call.platform, call.arch);
                if let Some(existing) = seen_libs.get_mut(&lib_key) {
                    if existing.0 == ToolScope::Optional && scope == ToolScope::RequiredForBuild {
                        *existing = (scope, ev);
                    }
                } else {
                    seen_libs.insert(lib_key, (scope, ev));
                }
            }

            // Extract find_program(...) calls
            let prog_calls = find_meson_calls(&content, "find_program");
            for call in prog_calls {
                if let Some(req) = parse_meson_find_program(&call, rel_path, &meson_options) {
                    evidence.push(req.evidence.clone());
                    requirements.push(req);
                }
            }
        }
    }

    if has_python {
        if let Some(ev) = python_evidence {
            requirements.push(ProjectRequirement::new(
                "python",
                RequirementKind::Runtime {
                    name: "python".to_string(),
                    constraint: VersionConstraint::GreaterEqual("3.0".to_string()),
                },
                ev.clone(),
            ));
            evidence.push(ev);
        }

        for (mod_name, (rel_path, line_no)) in seen_modules {
            let mod_ev = Evidence::new(
                EvidenceSource::BuildConfiguration {
                    path: rel_path,
                    line: Some(line_no),
                    detail: Some(format!("Python module '{}' required for build", mod_name)),
                },
                Confidence::High,
                format!("Python build module '{}' required by buildtools", mod_name),
            );
            requirements.push(ProjectRequirement::new(
                format!("python:{}", mod_name),
                RequirementKind::LanguagePackage {
                    language: "python".to_string(),
                    package: mod_name,
                    constraint: None,
                    scope: ToolScope::RequiredForBuild,
                },
                mod_ev.clone(),
            ));
            evidence.push(mod_ev);
        }
    }

    for ((name, plat, arch), (scope, ev)) in seen_libs {
        let mut req = ProjectRequirement::new(
            name.clone(),
            RequirementKind::SystemLibrary {
                name,
                header: None,
                constraint: None,
                scope,
            },
            ev.clone(),
        );
        if let Some(p) = plat {
            req = req.with_platform(p);
        }
        if let Some(a) = arch {
            req = req.with_arch(a);
        }
        requirements.push(req);
        evidence.push(ev);
    }

    MesonDiscovery {
        is_meson,
        languages,
        requirements,
        evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_analyze_meson_basic() {
        let dir = tempdir().unwrap();
        let meson_content = r#"
project('my_project', 'c',
        version: '1.0.0',
        meson_version: '>= 0.58.0',
        default_options: ['c_std=c11']
)
"#;
        fs::write(dir.path().join("meson.build"), meson_content).unwrap();

        let disc = analyze_meson(dir.path());
        assert!(disc.is_meson);
        assert_eq!(disc.languages, vec!["c"]);

        // Verify meson build tool requirement
        let meson_req = disc
            .requirements
            .iter()
            .find(|r| r.name == "meson")
            .expect("meson requirement missing");
        match &meson_req.kind {
            RequirementKind::BuildTool {
                constraint, scope, ..
            } => {
                assert_eq!(*scope, ToolScope::RequiredForBuild);
                assert!(constraint.is_some());
            }
            _ => panic!("Expected BuildTool"),
        }

        // Verify ninja requirement
        assert!(disc.requirements.iter().any(|r| r.name == "ninja"));

        // Verify compiler requirement
        let c_req = disc
            .requirements
            .iter()
            .find(|r| r.name == "c")
            .expect("c requirement missing");
        match &c_req.kind {
            RequirementKind::Compiler {
                language,
                min_standard,
                ..
            } => {
                assert_eq!(language, "c");
                assert_eq!(min_standard.as_deref(), Some("c11"));
            }
            _ => panic!("Expected Compiler"),
        }
    }

    #[test]
    fn test_meson_find_program_code_generator_and_anyof() {
        let dir = tempfile::tempdir().unwrap();
        let content = r#"
project('app', 'c', meson_version: '>= 0.54.0')
bison = find_program('bison', required: true)
flex = find_program('flex', version: '>= 2.6.0', required: true)
yacc = find_program('byacc', 'yacc', required: false)
pkgconf = find_program('pkg-config', 'pkgconf', required: false)
"#;
        fs::write(dir.path().join("meson.build"), content).unwrap();

        let disc = analyze_meson(dir.path());

        // Single code generator: bison
        let bison_req = disc
            .requirements
            .iter()
            .find(|r| r.name == "bison")
            .expect("bison requirement");
        match &bison_req.kind {
            RequirementKind::CodeGenerator {
                name,
                scope,
                constraint,
            } => {
                assert_eq!(name, "bison");
                assert_eq!(*scope, ToolScope::RequiredForBuild);
                assert!(constraint.is_none());
            }
            _ => panic!("Expected CodeGenerator for bison"),
        }

        // Single versioned code generator: flex
        let flex_req = disc
            .requirements
            .iter()
            .find(|r| r.name == "flex")
            .expect("flex requirement");
        match &flex_req.kind {
            RequirementKind::CodeGenerator {
                name,
                scope,
                constraint,
            } => {
                assert_eq!(name, "flex");
                assert_eq!(*scope, ToolScope::RequiredForBuild);
                assert_eq!(
                    constraint.as_ref().unwrap(),
                    &VersionConstraint::parse(">= 2.6.0")
                );
            }
            _ => panic!("Expected CodeGenerator for flex"),
        }

        // AnyOf code generator: byacc or yacc (optional)
        let yacc_req = disc
            .requirements
            .iter()
            .find(|r| r.name == "byacc_executable")
            .expect("byacc_executable requirement");
        match &yacc_req.kind {
            RequirementKind::AnyOf {
                capability,
                alternatives,
                scope,
            } => {
                assert_eq!(capability, "byacc_executable");
                assert_eq!(*scope, ToolScope::Optional);
                assert_eq!(alternatives.len(), 2);
                assert!(alternatives.iter().any(|a| a.name == "byacc"));
                assert!(alternatives.iter().any(|a| a.name == "yacc"));
            }
            _ => panic!("Expected AnyOf for byacc_executable"),
        }
    }

    #[test]
    fn test_meson_arch_guards() {
        let dir = tempdir().unwrap();
        let content = r#"
project('arch_app', 'c', meson_version: '>= 0.54.0')
if host_machine.cpu_family() == 'x86_64'
    dep_x86 = dependency('special_x86_lib')
elif host_machine.cpu_family() == 'aarch64'
    dep_arm = dependency('special_arm_lib')
endif
"#;
        fs::write(dir.path().join("meson.build"), content).unwrap();

        let disc = analyze_meson(dir.path());
        let x86_lib = disc
            .requirements
            .iter()
            .find(|r| r.name == "special_x86_lib")
            .expect("special_x86_lib requirement");
        assert_eq!(x86_lib.arch.as_deref(), Some("x86_64"));

        let arm_lib = disc
            .requirements
            .iter()
            .find(|r| r.name == "special_arm_lib")
            .expect("special_arm_lib requirement");
        assert_eq!(arm_lib.arch.as_deref(), Some("aarch64"));
    }
}

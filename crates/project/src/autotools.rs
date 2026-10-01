use concord_core::evidence::{Evidence, EvidenceSource};
use concord_core::ir::{ProjectRequirement, RequirementKind, ToolScope};
use concord_core::Confidence;
use concord_core::VersionConstraint;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Discovery results for an Autotools / Autoconf project.
#[derive(Debug, Clone)]
pub struct AutotoolsDiscovery {
    pub is_autotools: bool,
    pub languages: Vec<String>,
    pub requirements: Vec<ProjectRequirement>,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone)]
struct MacroInvocation {
    name: String,
    args: Vec<String>,
    line: usize,
    raw: String,
}

fn clean_m4_arg(raw: &str) -> String {
    let mut s = raw.trim();
    while (s.starts_with('[') && s.ends_with(']'))
        || (s.starts_with('"') && s.ends_with('"'))
        || (s.starts_with('\'') && s.ends_with('\''))
    {
        if s.len() < 2 {
            break;
        }
        s = s[1..s.len() - 1].trim();
    }
    s.to_string()
}

fn is_recognized_macro(ident: &str) -> bool {
    matches!(
        ident,
        "AC_CHECK_LIB"
            | "AC_SEARCH_LIBS"
            | "PKG_CHECK_MODULES"
            | "AC_PROG_CC"
            | "AC_PROG_CC_C99"
            | "AC_PROG_CC_STDC"
            | "AM_PROG_CC_C_O"
            | "AC_PROG_CXX"
            | "AC_PROG_YACC"
            | "AC_PROG_LEX"
            | "PKG_PROG_PKG_CONFIG"
    )
}

fn is_nullary_macro(ident: &str) -> bool {
    matches!(
        ident,
        "AC_PROG_CC"
            | "AC_PROG_CC_C99"
            | "AC_PROG_CC_STDC"
            | "AM_PROG_CC_C_O"
            | "AC_PROG_CXX"
            | "AC_PROG_YACC"
            | "AC_PROG_LEX"
            | "PKG_PROG_PKG_CONFIG"
    )
}

fn parse_macro_invocations(content: &str) -> Vec<MacroInvocation> {
    let chars: Vec<char> = content.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut line = 1;
    let mut invocations = Vec::new();

    while i < len {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }

        // Skip shell comments (# ...)
        if c == '#' {
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Skip dnl comments
        if (i == 0 || chars[i - 1].is_whitespace())
            && i + 3 <= len
            && &content[i..i + 3] == "dnl"
            && (i + 3 == len || chars[i + 3].is_whitespace() || chars[i + 3] == '(')
        {
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Identifiers
        if c.is_ascii_alphabetic() || c == '_' {
            let start_line = line;
            let start_pos = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident = &content[start_pos..i];

            if is_recognized_macro(ident) {
                // Skip whitespace to check for '('
                let mut j = i;
                let mut paren_line = line;
                while j < len
                    && (chars[j] == ' ' || chars[j] == '\t' || chars[j] == '\r' || chars[j] == '\n')
                {
                    if chars[j] == '\n' {
                        paren_line += 1;
                    }
                    j += 1;
                }

                if j < len && chars[j] == '(' {
                    let macro_name = ident.to_string();
                    let call_start = start_pos;
                    let mut paren_depth = 1;
                    let mut bracket_depth = 0;
                    let mut in_dquote = false;
                    let mut k = j + 1;
                    let mut current_arg = String::new();
                    let mut args = Vec::new();
                    line = paren_line;

                    while k < len && paren_depth > 0 {
                        let kc = chars[k];
                        if kc == '\n' {
                            line += 1;
                        }

                        if kc == '"' && bracket_depth == 0 {
                            in_dquote = !in_dquote;
                            current_arg.push(kc);
                        } else if !in_dquote && kc == '[' {
                            bracket_depth += 1;
                            current_arg.push(kc);
                        } else if !in_dquote && kc == ']' {
                            if bracket_depth > 0 {
                                bracket_depth -= 1;
                            }
                            current_arg.push(kc);
                        } else if !in_dquote && bracket_depth == 0 && kc == '(' {
                            paren_depth += 1;
                            current_arg.push(kc);
                        } else if !in_dquote && bracket_depth == 0 && kc == ')' {
                            paren_depth -= 1;
                            if paren_depth == 0 {
                                args.push(clean_m4_arg(&current_arg));
                                current_arg.clear();
                                k += 1;
                                break;
                            } else {
                                current_arg.push(kc);
                            }
                        } else if !in_dquote && bracket_depth == 0 && paren_depth == 1 && kc == ','
                        {
                            args.push(clean_m4_arg(&current_arg));
                            current_arg.clear();
                        } else {
                            current_arg.push(kc);
                        }
                        k += 1;
                    }

                    let raw = content[call_start..k].to_string();
                    invocations.push(MacroInvocation {
                        name: macro_name,
                        args,
                        line: start_line,
                        raw,
                    });
                    i = k;
                    continue;
                } else if is_nullary_macro(ident) {
                    invocations.push(MacroInvocation {
                        name: ident.to_string(),
                        args: vec![],
                        line: start_line,
                        raw: ident.to_string(),
                    });
                }
            }
        }

        i += 1;
    }

    invocations
}

fn extract_assigned_no_var(action: &str) -> Option<String> {
    for line in action.lines() {
        for part in line.split(';') {
            let trimmed = part.trim();
            for pattern in &["=no", "=\"no\"", "='no'", "=0", "=\"0\""] {
                if let Some(idx) = trimmed.find(pattern) {
                    let left = trimmed[..idx].trim();
                    let var = left.split_whitespace().last().unwrap_or(left);
                    let var = var.trim_matches(|c: char| c == '$' || c == '{' || c == '}');
                    if !var.is_empty() && var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    {
                        return Some(var.to_string());
                    }
                }
            }
        }
    }
    None
}

fn is_var_aborting(var: &str, content: &str) -> bool {
    let needle1 = format!("${}", var);
    let needle2 = format!("${{{}}}", var);

    for (line_idx, line) in content.lines().enumerate() {
        if line.contains(&needle1) || line.contains(&needle2) {
            let downstream = content
                .lines()
                .skip(line_idx)
                .take(80)
                .collect::<Vec<_>>()
                .join("\n");
            if downstream.contains("AC_MSG_ERROR") {
                return true;
            }
        }
    }
    false
}

fn parse_pkg_config_specs(spec_str: &str) -> Vec<(String, Option<VersionConstraint>)> {
    let mut results = Vec::new();
    let tokens: Vec<&str> = spec_str
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let ops = [">=", "<=", "==", "=", ">", "<"];
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        // Check if token contains operator directly, e.g. "foo>=1.0"
        let mut matched_op = false;
        for op in &ops {
            if let Some(pos) = token.find(op) {
                let pkg = token[..pos].trim();
                let ver = token[pos + op.len()..].trim();
                if !pkg.is_empty() && !ver.is_empty() {
                    let parse_str = if *op == "=" {
                        format!("=={}", ver)
                    } else {
                        format!("{}{}", op, ver)
                    };
                    results.push((pkg.to_string(), Some(VersionConstraint::parse(&parse_str))));
                    matched_op = true;
                    break;
                }
            }
        }
        if matched_op {
            i += 1;
            continue;
        }

        // Check if next token is an operator: "foo" ">=" "1.0"
        if i + 2 < tokens.len() && ops.contains(&tokens[i + 1]) {
            let pkg = token;
            let op = tokens[i + 1];
            let ver = tokens[i + 2];
            let parse_str = if op == "=" {
                format!("=={}", ver)
            } else {
                format!("{}{}", op, ver)
            };
            results.push((pkg.to_string(), Some(VersionConstraint::parse(&parse_str))));
            i += 3;
            continue;
        }

        // Check if next token starts with an operator: "foo" ">=1.0"
        if i + 1 < tokens.len() {
            let next_tok = tokens[i + 1];
            let mut found_next_op = false;
            for op in &ops {
                if let Some(stripped) = next_tok.strip_prefix(op) {
                    let ver = stripped.trim();
                    if !ver.is_empty() {
                        let parse_str = if *op == "=" {
                            format!("=={}", ver)
                        } else {
                            format!("{}{}", op, ver)
                        };
                        results.push((
                            token.to_string(),
                            Some(VersionConstraint::parse(&parse_str)),
                        ));
                        i += 2;
                        found_next_op = true;
                        break;
                    }
                }
            }
            if found_next_op {
                continue;
            }
        }

        // Plain package name without version constraint
        results.push((token.to_string(), None));
        i += 1;
    }
    results
}

#[derive(Debug, Clone)]
enum ExtractedItem {
    SystemLib {
        name: String,
        constraint: Option<VersionConstraint>,
        scope: ToolScope,
        line: usize,
        raw: String,
        status_var: Option<String>,
    },
    SearchLibs {
        symbol: String,
        libraries: Vec<String>,
        scope: ToolScope,
        line: usize,
        raw: String,
        status_var: Option<String>,
    },
}

/// Analyze Autotools configuration files (configure.ac, configure.in).
pub fn analyze_autotools(dir: &Path) -> AutotoolsDiscovery {
    let candidate_files = ["configure.ac", "configure.in"];
    let mut config_path = None;
    let mut config_filename = "configure.ac";

    for cf in &candidate_files {
        let p = dir.join(cf);
        if p.is_file() {
            config_path = Some(p);
            config_filename = cf;
            break;
        }
    }

    let config_path = match config_path {
        Some(p) => p,
        None => {
            return AutotoolsDiscovery {
                is_autotools: false,
                languages: vec![],
                requirements: vec![],
                evidence: vec![],
            };
        }
    };

    let content = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => {
            return AutotoolsDiscovery {
                is_autotools: true,
                languages: vec!["c".to_string()],
                requirements: vec![],
                evidence: vec![],
            };
        }
    };

    let invocations = parse_macro_invocations(&content);
    let mut languages = vec!["c".to_string()];
    let mut requirements = Vec::new();
    let mut evidence = Vec::new();
    let mut extracted_items = Vec::new();
    let mut has_pkg_config = false;
    let mut has_yacc = false;
    let mut has_lex = false;

    for inv in &invocations {
        match inv.name.as_str() {
            "AC_PROG_CC" | "AC_PROG_CC_C99" | "AC_PROG_CC_STDC" | "AM_PROG_CC_C_O" => {
                let ev = Evidence::new(
                    EvidenceSource::BuildConfiguration {
                        path: PathBuf::from(config_filename),
                        line: Some(inv.line),
                        detail: Some(format!(
                            "Autotools compiler declaration: {}",
                            inv.raw.trim()
                        )),
                    },
                    Confidence::Confirmed,
                    format!("C compiler declared via {} at line {}", inv.name, inv.line),
                );
                evidence.push(ev.clone());
                requirements.push(ProjectRequirement {
                    name: "c".to_string(),
                    kind: RequirementKind::Compiler {
                        language: "c".to_string(),
                        min_standard: None,
                        constraint: None,
                    },
                    evidence: ev,
                    additional_evidence: vec![],
                    platform: None,
                });
            }
            "AC_PROG_CXX" => {
                if !languages.contains(&"cpp".to_string()) {
                    languages.push("cpp".to_string());
                }
                let ev = Evidence::new(
                    EvidenceSource::BuildConfiguration {
                        path: PathBuf::from(config_filename),
                        line: Some(inv.line),
                        detail: Some(format!(
                            "Autotools C++ compiler declaration: {}",
                            inv.raw.trim()
                        )),
                    },
                    Confidence::Confirmed,
                    format!("C++ compiler declared via AC_PROG_CXX at line {}", inv.line),
                );
                evidence.push(ev.clone());
                requirements.push(ProjectRequirement {
                    name: "cpp".to_string(),
                    kind: RequirementKind::Compiler {
                        language: "cpp".to_string(),
                        min_standard: None,
                        constraint: None,
                    },
                    evidence: ev,
                    additional_evidence: vec![],
                    platform: None,
                });
            }
            "AC_PROG_YACC" => {
                has_yacc = true;
            }
            "AC_PROG_LEX" => {
                has_lex = true;
            }
            "PKG_PROG_PKG_CONFIG" => {
                has_pkg_config = true;
            }
            "AC_CHECK_LIB" => {
                if let Some(lib_name) = inv.args.first() {
                    let clean_lib = lib_name.trim();
                    let clean_lib = clean_lib.strip_prefix("-l").unwrap_or(clean_lib);
                    if !clean_lib.is_empty() {
                        let failure_branch = inv.args.get(3).map(|s| s.as_str()).unwrap_or("");
                        let status_var = extract_assigned_no_var(failure_branch);
                        let is_aborting = failure_branch.contains("AC_MSG_ERROR")
                            || status_var
                                .as_deref()
                                .map(|v| is_var_aborting(v, &content))
                                .unwrap_or(false);

                        let scope = if is_aborting {
                            ToolScope::RequiredForBuild
                        } else {
                            ToolScope::Optional
                        };

                        extracted_items.push(ExtractedItem::SystemLib {
                            name: clean_lib.to_string(),
                            constraint: None,
                            scope,
                            line: inv.line,
                            raw: inv.raw.clone(),
                            status_var,
                        });
                    }
                }
            }
            "AC_SEARCH_LIBS" => {
                let symbol = inv
                    .args
                    .first()
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                let libs_str = inv.args.get(1).map(|s| s.as_str()).unwrap_or("");
                let libs: Vec<String> = libs_str
                    .split(|c: char| c.is_whitespace() || c == ',')
                    .map(|s| {
                        s.trim_matches(|c: char| {
                            c == ',' || c == '[' || c == ']' || c == '"' || c == '\''
                        })
                        .to_string()
                    })
                    .map(|s| s.strip_prefix("-l").unwrap_or(&s).to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                if !libs.is_empty() {
                    let failure_branch = inv.args.get(3).map(|s| s.as_str()).unwrap_or("");
                    let status_var = extract_assigned_no_var(failure_branch);
                    let is_aborting = failure_branch.contains("AC_MSG_ERROR")
                        || status_var
                            .as_deref()
                            .map(|v| is_var_aborting(v, &content))
                            .unwrap_or(false);

                    let scope = if is_aborting {
                        ToolScope::RequiredForBuild
                    } else {
                        ToolScope::Optional
                    };

                    extracted_items.push(ExtractedItem::SearchLibs {
                        symbol,
                        libraries: libs,
                        scope,
                        line: inv.line,
                        raw: inv.raw.clone(),
                        status_var,
                    });
                }
            }
            "PKG_CHECK_MODULES" => {
                has_pkg_config = true;
                let spec_str = inv.args.get(1).map(|s| s.as_str()).unwrap_or("");
                let failure_branch = inv.args.get(3).map(|s| s.as_str()).unwrap_or("");
                let status_var = extract_assigned_no_var(failure_branch);
                let is_aborting = failure_branch.contains("AC_MSG_ERROR")
                    || status_var
                        .as_deref()
                        .map(|v| is_var_aborting(v, &content))
                        .unwrap_or(false);

                let scope = if is_aborting {
                    ToolScope::RequiredForBuild
                } else {
                    ToolScope::Optional
                };

                let parsed_specs = parse_pkg_config_specs(spec_str);
                for (pkg_name, constraint) in parsed_specs {
                    let clean_pkg = pkg_name.trim();
                    let clean_pkg = clean_pkg.strip_prefix("-l").unwrap_or(clean_pkg);
                    if !clean_pkg.is_empty() {
                        extracted_items.push(ExtractedItem::SystemLib {
                            name: clean_pkg.to_string(),
                            constraint,
                            scope,
                            line: inv.line,
                            raw: inv.raw.clone(),
                            status_var: status_var.clone(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    if has_yacc {
        let ev = Evidence::new(
            EvidenceSource::BuildConfiguration {
                path: PathBuf::from(config_filename),
                line: None,
                detail: Some("Parser generator declared via AC_PROG_YACC".to_string()),
            },
            Confidence::Confirmed,
            "Parser generator requirement (yacc/bison)",
        );
        evidence.push(ev.clone());
        let alternatives = vec!["bison", "yacc", "byacc"]
            .into_iter()
            .map(|tool| {
                let alt_ev = Evidence::new(
                    EvidenceSource::BuildConfiguration {
                        path: PathBuf::from(config_filename),
                        line: None,
                        detail: Some(format!("Parser generator alternative '{}'", tool)),
                    },
                    Confidence::Confirmed,
                    format!("Build tool alternative '{}'", tool),
                );
                ProjectRequirement::new(
                    tool,
                    RequirementKind::BuildTool {
                        name: tool.to_string(),
                        constraint: None,
                        scope: ToolScope::RequiredForBuild,
                    },
                    alt_ev,
                )
            })
            .collect();

        requirements.push(ProjectRequirement {
            name: "yacc_executable".to_string(),
            kind: RequirementKind::AnyOf {
                capability: "yacc_executable".to_string(),
                alternatives,
                scope: ToolScope::RequiredForBuild,
            },
            evidence: ev,
            additional_evidence: vec![],
            platform: None,
        });
    }

    if has_lex {
        let ev = Evidence::new(
            EvidenceSource::BuildConfiguration {
                path: PathBuf::from(config_filename),
                line: None,
                detail: Some("Lexer generator declared via AC_PROG_LEX".to_string()),
            },
            Confidence::Confirmed,
            "Lexer generator requirement (flex)",
        );
        evidence.push(ev.clone());
        requirements.push(ProjectRequirement {
            name: "flex".to_string(),
            kind: RequirementKind::CodeGenerator {
                name: "flex".to_string(),
                constraint: None,
                scope: ToolScope::RequiredForBuild,
            },
            evidence: ev,
            additional_evidence: vec![],
            platform: None,
        });
    }

    if has_pkg_config {
        let ev = Evidence::new(
            EvidenceSource::BuildConfiguration {
                path: PathBuf::from(config_filename),
                line: None,
                detail: Some("pkg-config tool declared in Autotools configuration".to_string()),
            },
            Confidence::Confirmed,
            "pkg-config tool capability",
        );
        evidence.push(ev.clone());
        let alternatives = vec!["pkg-config", "pkgconf"]
            .into_iter()
            .map(|tool| {
                let alt_ev = Evidence::new(
                    EvidenceSource::BuildConfiguration {
                        path: PathBuf::from(config_filename),
                        line: None,
                        detail: Some(format!("pkg-config alternative '{}'", tool)),
                    },
                    Confidence::Confirmed,
                    format!("Build tool alternative '{}'", tool),
                );
                ProjectRequirement::new(
                    tool,
                    RequirementKind::BuildTool {
                        name: tool.to_string(),
                        constraint: None,
                        scope: ToolScope::Optional,
                    },
                    alt_ev,
                )
            })
            .collect();

        requirements.push(ProjectRequirement {
            name: "pkg-config_executable".to_string(),
            kind: RequirementKind::AnyOf {
                capability: "pkg-config_executable".to_string(),
                alternatives,
                scope: ToolScope::Optional,
            },
            evidence: ev,
            additional_evidence: vec![],
            platform: None,
        });
    }

    // Group items that share an aborting status variable into AnyOf capabilities
    let mut grouped_by_status_var: HashMap<String, Vec<ExtractedItem>> = HashMap::new();
    let mut standalone_items = Vec::new();

    for item in extracted_items {
        let s_var = match &item {
            ExtractedItem::SystemLib { status_var, .. } => status_var.clone(),
            ExtractedItem::SearchLibs { status_var, .. } => status_var.clone(),
        };

        if let Some(v) = s_var {
            if is_var_aborting(&v, &content) {
                grouped_by_status_var.entry(v).or_default().push(item);
                continue;
            }
        }
        standalone_items.push(item);
    }

    // Emit grouped AnyOf requirements
    for (var, items) in grouped_by_status_var {
        if items.len() == 1 {
            // Only one item used this aborting status variable: treat as standalone requirement
            standalone_items.extend(items);
            continue;
        }

        let cap_name = var
            .strip_prefix("found_")
            .or_else(|| var.strip_prefix("have_"))
            .unwrap_or(&var);

        let mut alternatives = Vec::new();
        let mut first_line = 1;
        let mut first_raw = String::new();

        for item in items {
            match item {
                ExtractedItem::SystemLib {
                    name,
                    constraint,
                    scope,
                    line,
                    raw,
                    ..
                } => {
                    if first_raw.is_empty() {
                        first_line = line;
                        first_raw = raw.clone();
                    }
                    let alt_ev = Evidence::new(
                        EvidenceSource::BuildConfiguration {
                            path: PathBuf::from(config_filename),
                            line: Some(line),
                            detail: Some(format!(
                                "Disjunctive provider '{}': {}",
                                name,
                                raw.trim()
                            )),
                        },
                        Confidence::Confirmed,
                        format!(
                            "Alternative library '{}' for capability '{}'",
                            name, cap_name
                        ),
                    );
                    alternatives.push(ProjectRequirement::new(
                        name.clone(),
                        RequirementKind::SystemLibrary {
                            name,
                            header: None,
                            constraint,
                            scope,
                        },
                        alt_ev,
                    ));
                }
                ExtractedItem::SearchLibs {
                    symbol,
                    libraries,
                    scope,
                    line,
                    raw,
                    ..
                } => {
                    if first_raw.is_empty() {
                        first_line = line;
                        first_raw = raw.clone();
                    }
                    for lib in libraries {
                        let alt_ev = Evidence::new(
                            EvidenceSource::BuildConfiguration {
                                path: PathBuf::from(config_filename),
                                line: Some(line),
                                detail: Some(format!(
                                    "Disjunctive search library '{}' for symbol '{}'",
                                    lib, symbol
                                )),
                            },
                            Confidence::Confirmed,
                            format!(
                                "Alternative library '{}' for capability '{}'",
                                lib, cap_name
                            ),
                        );
                        alternatives.push(ProjectRequirement::new(
                            lib.clone(),
                            RequirementKind::SystemLibrary {
                                name: lib,
                                header: None,
                                constraint: None,
                                scope,
                            },
                            alt_ev,
                        ));
                    }
                }
            }
        }

        if !alternatives.is_empty() {
            let ev = Evidence::new(
                EvidenceSource::BuildConfiguration {
                    path: PathBuf::from(config_filename),
                    line: Some(first_line),
                    detail: Some(format!(
                        "Autotools disjunctive dependency chain: {}",
                        first_raw.lines().next().unwrap_or(&first_raw).trim()
                    )),
                },
                Confidence::Confirmed,
                format!(
                    "Disjunctive provider for capability '{}' at line {}",
                    cap_name, first_line
                ),
            );
            evidence.push(ev.clone());
            requirements.push(ProjectRequirement {
                name: cap_name.to_string(),
                kind: RequirementKind::AnyOf {
                    capability: cap_name.to_string(),
                    alternatives,
                    scope: ToolScope::RequiredForBuild,
                },
                evidence: ev,
                additional_evidence: vec![],
                platform: None,
            });
        }
    }

    // Emit standalone items
    for item in standalone_items {
        match item {
            ExtractedItem::SystemLib {
                name,
                constraint,
                scope,
                line,
                raw,
                ..
            } => {
                let ev = Evidence::new(
                    EvidenceSource::BuildConfiguration {
                        path: PathBuf::from(config_filename),
                        line: Some(line),
                        detail: Some(format!(
                            "Autotools library requirement: {}",
                            raw.lines().next().unwrap_or(&raw).trim()
                        )),
                    },
                    Confidence::Confirmed,
                    format!("System library '{}' declared at line {}", name, line),
                );
                evidence.push(ev.clone());
                requirements.push(ProjectRequirement {
                    name: name.clone(),
                    kind: RequirementKind::SystemLibrary {
                        name,
                        header: None,
                        constraint,
                        scope,
                    },
                    evidence: ev,
                    additional_evidence: vec![],
                    platform: None,
                });
            }
            ExtractedItem::SearchLibs {
                symbol,
                libraries,
                scope,
                line,
                raw,
                ..
            } => {
                let cap_name = if symbol.is_empty() {
                    libraries
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "lib".to_string())
                } else {
                    format!("{}_library", symbol)
                };

                let alternatives: Vec<ProjectRequirement> = libraries
                    .into_iter()
                    .map(|lib| {
                        let alt_ev = Evidence::new(
                            EvidenceSource::BuildConfiguration {
                                path: PathBuf::from(config_filename),
                                line: Some(line),
                                detail: Some(format!(
                                    "Alternative library '{}' for symbol '{}'",
                                    lib, symbol
                                )),
                            },
                            Confidence::Confirmed,
                            format!("Alternative library '{}' for symbol '{}'", lib, symbol),
                        );
                        ProjectRequirement::new(
                            lib.clone(),
                            RequirementKind::SystemLibrary {
                                name: lib,
                                header: None,
                                constraint: None,
                                scope,
                            },
                            alt_ev,
                        )
                    })
                    .collect();

                let ev = Evidence::new(
                    EvidenceSource::BuildConfiguration {
                        path: PathBuf::from(config_filename),
                        line: Some(line),
                        detail: Some(format!(
                            "Autotools AC_SEARCH_LIBS requirement: {}",
                            raw.lines().next().unwrap_or(&raw).trim()
                        )),
                    },
                    Confidence::Confirmed,
                    format!("Search libraries for symbol '{}' at line {}", symbol, line),
                );
                evidence.push(ev.clone());
                requirements.push(ProjectRequirement {
                    name: cap_name.clone(),
                    kind: RequirementKind::AnyOf {
                        capability: cap_name,
                        alternatives,
                        scope,
                    },
                    evidence: ev,
                    additional_evidence: vec![],
                    platform: None,
                });
            }
        }
    }

    AutotoolsDiscovery {
        is_autotools: true,
        languages,
        requirements,
        evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_case_1_ac_check_lib_required() {
        let dir = tempfile::tempdir().unwrap();
        let ac_content = "AC_INIT([test_proj], [1.0])\nAC_PROG_CC\nAC_CHECK_LIB(testlib, test_sym, [], [AC_MSG_ERROR([testlib is required])])\n";
        std::fs::write(dir.path().join("configure.ac"), ac_content).unwrap();

        let disc = analyze_autotools(dir.path());
        assert!(disc.is_autotools);
        assert_eq!(disc.languages, vec!["c".to_string()]);

        let req = disc
            .requirements
            .iter()
            .find(|r| r.name == "testlib")
            .expect("testlib requirement");
        match &req.kind {
            RequirementKind::SystemLibrary { name, scope, .. } => {
                assert_eq!(name, "testlib");
                assert_eq!(*scope, ToolScope::RequiredForBuild);
            }
            _ => panic!("expected SystemLibrary requirement"),
        }
    }

    #[test]
    fn test_case_2_ac_search_libs_anyof() {
        let dir = tempfile::tempdir().unwrap();
        let ac_content =
            "AC_INIT([test_proj], [1.0])\nAC_SEARCH_LIBS(search_sym, [provider_a provider_b])\n";
        std::fs::write(dir.path().join("configure.ac"), ac_content).unwrap();

        let disc = analyze_autotools(dir.path());
        assert!(disc.is_autotools);

        let req = disc
            .requirements
            .iter()
            .find(|r| matches!(&r.kind, RequirementKind::AnyOf { capability, .. } if capability == "search_sym_library"))
            .expect("search_sym_library AnyOf");

        match &req.kind {
            RequirementKind::AnyOf {
                capability,
                alternatives,
                scope,
            } => {
                assert_eq!(capability, "search_sym_library");
                assert_eq!(*scope, ToolScope::Optional);
                assert_eq!(alternatives.len(), 2);
                let alt_names: Vec<&str> = alternatives
                    .iter()
                    .filter_map(|a| match &a.kind {
                        RequirementKind::SystemLibrary { name, .. } => Some(name.as_str()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(alt_names, vec!["provider_a", "provider_b"]);
            }
            _ => panic!("expected AnyOf requirement"),
        }
    }

    #[test]
    fn test_case_3_pkg_check_modules_versioned() {
        let dir = tempfile::tempdir().unwrap();
        let ac_content = "AC_INIT([test_proj], [1.0])\nPKG_CHECK_MODULES([FOO], [foo >= 2.0])\n";
        std::fs::write(dir.path().join("configure.ac"), ac_content).unwrap();

        let disc = analyze_autotools(dir.path());
        assert!(disc.is_autotools);

        let req = disc
            .requirements
            .iter()
            .find(|r| r.name == "foo")
            .expect("foo requirement");
        match &req.kind {
            RequirementKind::SystemLibrary {
                name,
                constraint,
                scope,
                ..
            } => {
                assert_eq!(name, "foo");
                assert_eq!(*scope, ToolScope::Optional);
                assert_eq!(
                    constraint.as_ref().unwrap(),
                    &VersionConstraint::GreaterEqual("2.0".to_string())
                );
            }
            _ => panic!("expected SystemLibrary requirement"),
        }
    }

    #[test]
    fn test_case_4_ac_msg_error_turns_probed_dependency_mandatory() {
        let dir = tempfile::tempdir().unwrap();
        let ac_content = "AC_INIT([test_proj], [1.0])\nAC_CHECK_LIB(bar, bar_sym, [found_bar=yes], [found_bar=no])\nif test \"x$found_bar\" = \"xno\"; then\n    AC_MSG_ERROR([bar library missing])\nfi\n";
        std::fs::write(dir.path().join("configure.ac"), ac_content).unwrap();

        let disc = analyze_autotools(dir.path());
        assert!(disc.is_autotools);

        let req = disc
            .requirements
            .iter()
            .find(|r| r.name == "bar")
            .expect("bar requirement");
        match &req.kind {
            RequirementKind::SystemLibrary { name, scope, .. } => {
                assert_eq!(name, "bar");
                assert_eq!(*scope, ToolScope::RequiredForBuild);
            }
            _ => panic!("expected SystemLibrary requirement with RequiredForBuild scope"),
        }
    }
}

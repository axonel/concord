use colored::*;
use concord_constraints::model::EvaluatedConstraint;
use concord_core::Confidence;
use concord_diagnosis::Diagnosis;
use concord_predictor::Prediction;
use concord_verifier::VerificationReport;

pub fn print_banner() {
    println!(
        "{}",
        "CONCORD — Development Environment Engine".bold().cyan()
    );
    println!("{}", "──────────────────────────────────────────".dimmed());
}

pub fn print_human_summary(
    project: &concord_core::ir::ProjectManifest,
    project_path: &str,
    predictions: &[Prediction],
    evaluated_constraints: &[EvaluatedConstraint],
    verbose: bool,
) {
    print_banner();
    println!("Project:     {}", project_path.bold());
    println!("Name:        {}", project.name);
    if !project.languages.is_empty() {
        println!("Languages:   {}", project.languages.join(", "));
    }
    if !project.package_managers.is_empty() {
        println!("Package Mgr: {}", project.package_managers.join(", "));
    }
    let tools: Vec<&str> = project
        .requirements
        .iter()
        .filter_map(|r| match &r.kind {
            concord_core::ir::RequirementKind::DeveloperTool { name, .. }
            | concord_core::ir::RequirementKind::BuildTool { name, .. }
            | concord_core::ir::RequirementKind::CodeGenerator { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    if !tools.is_empty() {
        let mut uniq_tools = tools;
        uniq_tools.sort();
        uniq_tools.dedup();
        println!("Tools:       {}", uniq_tools.join(", "));
    }
    if !project.components.is_empty() {
        let comp_strs: Vec<String> = project
            .components
            .iter()
            .map(|c| format!("{} ({})", c.name.bold(), c.languages.join("/")))
            .collect();
        println!("Components:  {}", comp_strs.join(", "));
    }
    if !project.declared_ports.is_empty() {
        let ports_str: Vec<String> = project
            .declared_ports
            .iter()
            .map(|p| p.to_string())
            .collect();
        println!("Ports:       {}", ports_str.join(", "));
    }
    if !project.env_var_specs.is_empty() {
        let required_cnt = project
            .env_var_specs
            .iter()
            .filter(|s| matches!(s.category, concord_core::ir::EnvVarCategory::Required))
            .count();
        let optional_cnt = project
            .env_var_specs
            .iter()
            .filter(|s| {
                matches!(
                    s.category,
                    concord_core::ir::EnvVarCategory::OptionalWithDefault
                )
            })
            .count();
        let local_cnt = project
            .env_var_specs
            .iter()
            .filter(|s| matches!(s.category, concord_core::ir::EnvVarCategory::ConfiguredLocal))
            .count();

        let mut parts = Vec::new();
        if required_cnt > 0 {
            parts.push(format!("{} required", required_cnt));
        }
        if optional_cnt > 0 {
            parts.push(format!("{} with defaults", optional_cnt));
        }
        if local_cnt > 0 {
            parts.push(format!("{} in local .env", local_cnt));
        }
        if !parts.is_empty() {
            println!("Environment: {}", parts.join(", "));
        }
    }
    if !project.compose_projects.is_empty() {
        let compose_strs: Vec<String> = project
            .compose_projects
            .iter()
            .map(|cp| {
                let srvs: Vec<&str> = cp.services.iter().map(|s| s.name.as_str()).collect();
                format!("{} ({})", cp.file_path.display(), srvs.join(", "))
            })
            .collect();
        println!("Compose:     {}", compose_strs.join("; "));
    }
    println!();

    if predictions.is_empty() {
        println!("{}", "Environment: COMPATIBLE".bold().green());
        println!();
        println!(" {} Project requirements discovered", "✓".green());
        println!(" {} Runtime versions compatible", "✓".green());
        println!(" {} Required services available", "✓".green());
        println!(" {} Declared ports available", "✓".green());
        println!(" {} Configuration consistent", "✓".green());
        println!();
        println!("{}", "No known blockers.".green());
    } else {
        let violations_count = evaluated_constraints
            .iter()
            .filter(|c| c.is_violated())
            .count();
        println!(
            "{}",
            format!(
                "{} problems detected ({} predicted failures)",
                violations_count,
                predictions.len()
            )
            .bold()
            .red()
        );
        println!();

        for pred in predictions {
            let conf_tag = match pred.confidence {
                Confidence::Confirmed => "CONFIRMED".bold().on_red().white(),
                Confidence::High => "HIGH     ".bold().red(),
                Confidence::Medium => "MEDIUM   ".bold().yellow(),
                Confidence::Low => "LOW      ".dimmed(),
                Confidence::Unknown => "UNKNOWN  ".dimmed(),
            };

            println!("  {}  {}", conf_tag, pred.title.bold());
            println!("             {}", pred.summary.dimmed());

            if verbose {
                if let Some(ref p_ev) = pred.project_evidence {
                    println!("             project evidence: {}", p_ev.description.cyan());
                }
                if let Some(ref m_ev) = pred.machine_evidence {
                    println!(
                        "             machine evidence: {}",
                        m_ev.description.magenta()
                    );
                }
            }
        }

        println!();
        println!("{}", "Next steps:".bold());
        println!("  concord explain     Inspect root causes and causal dependency chains");
        println!("  concord verify      Run read-only environment verification checks");
    }
}

pub fn print_diagnoses(diagnoses: &[Diagnosis], verbose: bool) {
    print_banner();
    println!("{}", "Root-Cause Diagnosis & Causal Chains".bold());
    println!();

    if diagnoses.is_empty() {
        println!(
            "{}",
            "No environment failures diagnosed. All invariants hold.".green()
        );
        return;
    }

    for (idx, diag) in diagnoses.iter().enumerate() {
        println!("{}. {}", idx + 1, diag.problem.bold().red());
        let root_cause_display =
            if let Some(stripped) = diag.root_cause.strip_prefix("missing.env_file:") {
                format!("missing {}", stripped)
            } else {
                diag.root_cause.clone()
            };
        println!(
            "   Root Cause:          {}",
            root_cause_display.bold().yellow()
        );
        println!("   Confidence:          {}", diag.confidence);
        if !diag.directly_affected_services.is_empty()
            || !diag.transitively_blocked_services.is_empty()
        {
            if !diag.directly_affected_services.is_empty() {
                println!(
                    "   Directly Affected:   {}",
                    diag.directly_affected_services.join(", ")
                );
            }
            if !diag.transitively_blocked_services.is_empty() {
                println!(
                    "   Transitively Blocked:{}",
                    diag.transitively_blocked_services.join(", ")
                );
            }
        } else if !diag.affected_services.is_empty() {
            println!(
                "   Affected Services:   {}",
                diag.affected_services.join(", ")
            );
        } else {
            println!("   Violated Constraint: {}", diag.violated_constraint);
            println!(
                "   Affected Components: {}",
                diag.affected_components.join(", ")
            );
        }
        if let Some(ref tpl) = diag.configuration_template {
            println!(
                "   Configuration template found: {}",
                tpl.display().to_string().cyan()
            );
        }
        for suggestion in &diag.bootstrap_suggestions {
            println!("   Bootstrap Action:    {}", suggestion.cyan());
        }
        if verbose
            && (!diag.affected_services.is_empty() || !diag.directly_affected_services.is_empty())
        {
            println!("   Violated Constraint: {}", diag.violated_constraint);
        }
        println!();
        println!("   Causal Chain:");
        for (step_num, step) in diag.causal_chain.iter().enumerate() {
            if step_num == diag.causal_chain.len() - 1 {
                println!("     └─► {}", step.red());
            } else {
                println!("     ├─► {}", step);
            }
        }
        println!();

        if verbose {
            if let Some(ref p_ev) = diag.project_evidence {
                println!(
                    "   Project Evidence:    {} ({:?})",
                    p_ev.description, p_ev.source
                );
            }
            if let Some(ref m_ev) = diag.machine_evidence {
                println!(
                    "   Machine Evidence:    {} ({:?})",
                    m_ev.description, m_ev.source
                );
            }
            println!();
        }
    }
}

pub fn print_verification(report: &VerificationReport) {
    print_banner();
    println!("{}", "Read-Only Environment Verification".bold());
    println!();

    for check in &report.checks {
        let mark = if check.passed {
            "✓".green()
        } else {
            "✗".red()
        };
        let status = if check.passed {
            "PASSED".green()
        } else {
            "FAILED".bold().red()
        };

        println!(
            "  {} [{}] {:<25} {}",
            mark, status, check.name, check.message
        );
    }

    println!();
    println!(
        "Summary: {}/{} checks passed ({} failed)",
        report.passed_checks, report.total_checks, report.failed_checks
    );

    if report.success {
        println!("{}", "Environment verification PASSED.".bold().green());
    } else {
        println!("{}", "Environment verification FAILED.".bold().red());
    }
}

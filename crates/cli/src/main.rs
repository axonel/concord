use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use concord::format;
use concord::{execute_pipeline, ConcordReport};
use concord_diagnosis::Diagnosis;
use concord_project::analyze_project;

#[derive(Parser)]
#[command(
    name = "concord",
    author = "Axonel Team",
    version,
    about = "Development-environment resolution engine",
    long_about = "CONCORD models development environments as constraint systems, predicting failures before they happen and explaining root causes with structured evidence."
)]
pub struct Cli {
    /// Path to target project repository
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Output results in structured JSON format
    #[arg(long, global = true)]
    pub json: bool,

    /// Verbose output with full evidence and provenance details
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Scan project and host machine without evaluating predictions
    Scan {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Predict runtime and environment failures before execution
    Predict {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Explain root causes and causal dependency chains
    Explain {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        target: Option<String>,
    },
    /// Perform read-only verification of environment against project requirements
    Verify {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

const EXIT_OK: u8 = 0;
const EXIT_PROBLEMS: u8 = 1;
const EXIT_ERROR: u8 = 2;

fn handle_error(err: &impl std::fmt::Display, json: bool) -> ExitCode {
    if json {
        let err_obj = serde_json::json!({
            "error": err.to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&err_obj).unwrap());
    } else {
        eprintln!("Error: {}", err);
    }
    ExitCode::from(EXIT_ERROR)
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Scan { path }) => match analyze_project(path) {
            Ok(proj) => {
                let mach = concord_scanner::scan_machine_for_project(Some(path));
                if cli.json {
                    let scan_json = serde_json::json!({
                        "project": proj,
                        "machine": mach,
                    });
                    println!("{}", serde_json::to_string_pretty(&scan_json).unwrap());
                } else {
                    format::print_banner();
                    println!("Project: {}", path.display());
                    println!("Languages: {:?}", proj.languages);
                    println!("Requirements: {}", proj.requirements.len());
                    println!("Machine OS: {}", mach.os);
                    println!("Machine Arch: {}", mach.arch);
                    println!("Discovered runtimes: {}", mach.runtimes.len());
                    for r in &mach.runtimes {
                        println!(
                            "  - {} {} ({})",
                            r.name,
                            r.version,
                            r.executable_path.display()
                        );
                    }
                    if !mach.package_managers.is_empty() {
                        println!(
                            "Discovered package managers: {}",
                            mach.package_managers.len()
                        );
                        for pm in &mach.package_managers {
                            let ver_str = pm.version.as_deref().unwrap_or("unknown");
                            println!(
                                "  - {} {} ({})",
                                pm.name,
                                ver_str,
                                pm.executable_path.display()
                            );
                        }
                    }
                    if !mach.tools.is_empty() {
                        println!("Discovered tools: {}", mach.tools.len());
                        for t in &mach.tools {
                            let ver_str = t.version.as_deref().unwrap_or("present");
                            println!(
                                "  - {} {} ({})",
                                t.name,
                                ver_str,
                                t.executable_path.display()
                            );
                        }
                    }
                }
                ExitCode::from(EXIT_OK)
            }
            Err(e) => handle_error(&e, cli.json),
        },

        Some(Commands::Predict { path }) => match execute_pipeline(path) {
            Ok(out) => {
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&out.predictions).unwrap()
                    );
                } else {
                    format::print_banner();
                    println!("Predicted Failures: {}", out.predictions.len());
                    for pred in &out.predictions {
                        println!("- [{}] {}: {}", pred.confidence, pred.title, pred.summary);
                    }
                }
                if out.predictions.is_empty() {
                    ExitCode::from(EXIT_OK)
                } else {
                    ExitCode::from(EXIT_PROBLEMS)
                }
            }
            Err(e) => handle_error(&e, cli.json),
        },

        Some(Commands::Explain { path, target }) => match execute_pipeline(path) {
            Ok(out) => {
                let filtered: Vec<Diagnosis> = if let Some(t) = target {
                    out.diagnoses
                        .into_iter()
                        .filter(|d| {
                            d.affected_components.iter().any(|c| c.contains(t))
                                || d.problem.to_lowercase().contains(&t.to_lowercase())
                        })
                        .collect()
                } else {
                    out.diagnoses
                };

                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&filtered).unwrap());
                } else {
                    format::print_diagnoses(&filtered, cli.verbose);
                }
                ExitCode::from(EXIT_OK)
            }
            Err(e) => handle_error(&e, cli.json),
        },

        Some(Commands::Verify { path }) => match execute_pipeline(path) {
            Ok(out) => {
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&out.verification).unwrap()
                    );
                } else {
                    format::print_verification(&out.verification);
                }
                if out.verification.success {
                    ExitCode::from(EXIT_OK)
                } else {
                    ExitCode::from(EXIT_PROBLEMS)
                }
            }
            Err(e) => handle_error(&e, cli.json),
        },

        None => {
            // Default command: concord [PATH]
            match execute_pipeline(&cli.path) {
                Ok(out) => {
                    let has_failures = !out.predictions.is_empty();
                    if cli.json {
                        let report = ConcordReport {
                            project: out.env_model.project,
                            machine: out.env_model.machine,
                            evaluated_constraints: out.evaluated_constraints,
                            predictions: out.predictions,
                            diagnoses: out.diagnoses,
                            verification: out.verification,
                        };
                        println!("{}", serde_json::to_string_pretty(&report).unwrap());
                    } else {
                        format::print_human_summary(
                            &out.env_model.project,
                            &cli.path.display().to_string(),
                            &out.predictions,
                            &out.evaluated_constraints,
                            cli.verbose,
                        );
                    }

                    if has_failures {
                        ExitCode::from(EXIT_PROBLEMS)
                    } else {
                        ExitCode::from(EXIT_OK)
                    }
                }
                Err(e) => handle_error(&e, cli.json),
            }
        }
    }
}

use concord_constraints::evaluator::evaluate_all;
use concord_core::ir::{EnvironmentModel, PortInfo, PortState};
use concord_core::Confidence;
use concord_diagnosis::diagnose_all;
use concord_graph::EnvironmentGraph;
use concord_predictor::{predict_failures, PredictionCategory};
use concord_project::analyze_project;
use concord_scanner::scan_machine;
use concord_verifier::verify_environment;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
}

#[test]
fn test_fixture_analysis_node() {
    let fixture_path = fixtures_dir().join("healthy-node-app");
    let manifest = analyze_project(&fixture_path).expect("analyze healthy-node-app");

    assert_eq!(manifest.name, "healthy-node-app");
    assert!(manifest
        .languages
        .contains(&"javascript/typescript".to_string()));
    let node_req = manifest.requirements.iter().find(|r| r.name == "node");
    assert!(node_req.is_some());
}

#[test]
fn test_fixture_analysis_python() {
    let fixture_path = fixtures_dir().join("healthy-python-app");
    let manifest = analyze_project(&fixture_path).expect("analyze healthy-python-app");

    assert_eq!(manifest.name, "healthy-python-app");
    assert!(manifest.languages.contains(&"python".to_string()));
    let py_req = manifest.requirements.iter().find(|r| r.name == "python");
    assert!(py_req.is_some());
}

#[test]
fn test_fixture_analysis_docker() {
    let fixture_path = fixtures_dir().join("docker-postgres-app");
    let manifest = analyze_project(&fixture_path).expect("analyze docker-postgres-app");

    assert!(manifest.docker_used);
    assert!(manifest.declared_ports.contains(&3000));
    assert!(manifest.declared_ports.contains(&5432));
    assert!(manifest.requirements.iter().any(|r| r.name == "docker"));
    assert!(manifest.requirements.iter().any(|r| r.name == "postgresql"));
}

#[test]
fn test_broken_node_version_prediction() {
    let fixture_path = fixtures_dir().join("broken-node-version");
    let manifest = analyze_project(&fixture_path).expect("analyze broken-node-version");
    let machine = scan_machine();

    let evaluated_constraints = evaluate_all(&manifest.requirements, &machine);
    let env_model = EnvironmentModel::new(manifest, machine);
    let graph = EnvironmentGraph::build(&env_model, &evaluated_constraints);
    let predictions = predict_failures(&env_model, &evaluated_constraints);
    let traces = graph.all_causal_traces();
    let diagnoses = diagnose_all(&predictions, &traces);

    // Because broken-node-version requires node >= 99.0.0, this MUST be predicted as a failure
    assert!(
        !predictions.is_empty(),
        "Should predict failure for node >= 99.0.0"
    );
    let node_pred = predictions
        .iter()
        .find(|p| p.category == PredictionCategory::RuntimeIncompatibility);
    assert!(node_pred.is_some());
    let pred = node_pred.unwrap();
    assert_eq!(pred.confidence, Confidence::High);

    // Verify diagnosis
    let node_diag = diagnoses.iter().find(|d| d.problem.contains("node"));
    assert!(node_diag.is_some());
    let diag = node_diag.unwrap();
    assert!(diag.root_cause.contains("node.version"));
    assert_eq!(diag.causal_chain.len(), 4);
}

#[test]
fn test_broken_python_version_prediction() {
    let fixture_path = fixtures_dir().join("broken-python-version");
    let manifest = analyze_project(&fixture_path).expect("analyze broken-python-version");
    let machine = scan_machine();

    let evaluated_constraints = evaluate_all(&manifest.requirements, &machine);
    let env_model = EnvironmentModel::new(manifest, machine);
    let graph = EnvironmentGraph::build(&env_model, &evaluated_constraints);
    let predictions = predict_failures(&env_model, &evaluated_constraints);
    let traces = graph.all_causal_traces();
    let diagnoses = diagnose_all(&predictions, &traces);

    // broken-python-version requires python >= 3.99.0
    assert!(
        !predictions.is_empty(),
        "Should predict failure for python >= 3.99.0"
    );
    let py_pred = predictions
        .iter()
        .find(|p| p.category == PredictionCategory::RuntimeIncompatibility);
    assert!(py_pred.is_some());
    assert_eq!(py_pred.unwrap().confidence, Confidence::High);

    let py_diag = diagnoses.iter().find(|d| d.problem.contains("python"));
    assert!(py_diag.is_some());
    assert!(py_diag.unwrap().root_cause.contains("python.version"));
}

#[test]
fn test_port_collision_prediction() {
    let fixture_path = fixtures_dir().join("docker-postgres-app");
    let manifest = analyze_project(&fixture_path).expect("analyze docker-postgres-app");

    // Create a mock machine where port 3000 is occupied
    let mut machine = scan_machine();
    machine.listening_ports.push(PortInfo {
        port: 3000,
        state: PortState::Occupied {
            pid: Some(1337),
            process_name: Some("rogue-web".to_string()),
        },
        evidence: concord_core::evidence::Evidence::new(
            concord_core::evidence::EvidenceSource::ProcessInspection {
                pid: 1337,
                name: "rogue-web".to_string(),
                cmdline: None,
            },
            Confidence::Confirmed,
            "Port 3000 occupied by rogue-web",
        ),
    });

    let evaluated_constraints = evaluate_all(&manifest.requirements, &machine);
    let env_model = EnvironmentModel::new(manifest, machine);
    let graph = EnvironmentGraph::build(&env_model, &evaluated_constraints);
    let predictions = predict_failures(&env_model, &evaluated_constraints);
    let traces = graph.all_causal_traces();
    let diagnoses = diagnose_all(&predictions, &traces);

    let port_pred = predictions
        .iter()
        .find(|p| p.category == PredictionCategory::PortCollision);
    assert!(port_pred.is_some(), "Port collision should be predicted");
    assert_eq!(port_pred.unwrap().confidence, Confidence::High);

    let port_diag = diagnoses.iter().find(|d| d.problem.contains("3000"));
    assert!(port_diag.is_some());
    assert!(port_diag.unwrap().root_cause.contains("port:3000.free"));
}

#[test]
fn test_read_only_verification_report() {
    let fixture_path = fixtures_dir().join("broken-node-version");
    let manifest = analyze_project(&fixture_path).expect("analyze broken-node-version");
    let machine = scan_machine();
    let evaluated_constraints = evaluate_all(&manifest.requirements, &machine);
    let env_model = EnvironmentModel::new(manifest, machine);

    let report = verify_environment(&env_model, &evaluated_constraints);
    assert!(!report.success);
    assert!(report.failed_checks >= 1);
}

#[test]
fn test_json_serialization_roundtrip() {
    let fixture_path = fixtures_dir().join("healthy-node-app");
    let manifest = analyze_project(&fixture_path).expect("analyze healthy-node-app");
    let machine = scan_machine();
    let evaluated_constraints = evaluate_all(&manifest.requirements, &machine);
    let env_model = EnvironmentModel::new(manifest.clone(), machine.clone());
    let predictions = predict_failures(&env_model, &evaluated_constraints);
    let graph = EnvironmentGraph::build(&env_model, &evaluated_constraints);
    let traces = graph.all_causal_traces();
    let diagnoses = diagnose_all(&predictions, &traces);
    let verification = verify_environment(&env_model, &evaluated_constraints);

    let report = concord::ConcordReport {
        project: manifest,
        machine,
        evaluated_constraints,
        predictions,
        diagnoses,
        verification,
    };

    let json_str = serde_json::to_string_pretty(&report).expect("serialize report");
    assert!(json_str.contains("healthy-node-app"));

    // Verify it parses back as generic Value and retains keys
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("deserialize report");
    assert!(parsed.get("project").is_some());
    assert!(parsed.get("machine").is_some());
    assert!(parsed.get("predictions").is_some());
    assert!(parsed.get("diagnoses").is_some());
    assert!(parsed.get("verification").is_some());
}

#[test]
fn test_fixture_conflict_node_version() {
    let fixture_path = fixtures_dir().join("conflict-node-version");
    let manifest = analyze_project(&fixture_path).expect("analyze conflict-node-version");
    let machine = scan_machine();

    let evaluated_constraints = evaluate_all(&manifest.requirements, &machine);
    let env_model = EnvironmentModel::new(manifest, machine);
    let predictions = predict_failures(&env_model, &evaluated_constraints);

    let conflict_pred = predictions
        .iter()
        .find(|p| p.category == PredictionCategory::ConfigurationConflict);
    assert!(
        conflict_pred.is_some(),
        "Expected ConfigurationConflict prediction for node version mismatch between .nvmrc and package.json"
    );
    let pred = conflict_pred.unwrap();
    assert_eq!(pred.confidence, Confidence::Confirmed);
    assert!(pred
        .summary
        .contains("Contradictory node version requirements"));
}

#[test]
fn test_fixture_missing_env_app() {
    let fixture_path = fixtures_dir().join("missing-env-app");
    let manifest = analyze_project(&fixture_path).expect("analyze missing-env-app");
    let machine = scan_machine();

    let evaluated_constraints = evaluate_all(&manifest.requirements, &machine);
    let env_model = EnvironmentModel::new(manifest, machine);
    let predictions = predict_failures(&env_model, &evaluated_constraints);

    // API_SECRET_KEY is required and missing
    let secret_pred = predictions
        .iter()
        .find(|p| p.title.contains("API_SECRET_KEY"));
    assert!(
        secret_pred.is_some(),
        "Expected missing env prediction for API_SECRET_KEY"
    );

    // PORT has default 3000, so it should NOT be flagged as missing
    let port_pred = predictions.iter().find(|p| p.title.contains("PORT"));
    assert!(
        port_pred.is_none(),
        "PORT has a default in .env.example and should not be predicted as missing"
    );
}

#[test]
fn test_fixture_mise_pinned_tools() {
    let fixture_path = fixtures_dir().join("mise-pinned-tools");
    let manifest = analyze_project(&fixture_path).expect("analyze mise-pinned-tools");

    // Check package manager consolidation (pnpm from package.json engines + packageManager + mise.toml)
    let pnpm_reqs: Vec<_> = manifest
        .requirements
        .iter()
        .filter(|r| r.name == "pnpm")
        .collect();
    assert_eq!(
        pnpm_reqs.len(),
        1,
        "pnpm requirements must be consolidated into exactly one requirement"
    );
    let pnpm_req = pnpm_reqs[0];
    match &pnpm_req.kind {
        concord_core::ir::RequirementKind::PackageManager { name, constraint } => {
            assert_eq!(name, "pnpm");
            assert_eq!(
                constraint,
                &Some(concord_core::version::VersionConstraint::Exact(
                    "11.24.0".to_string()
                ))
            );
        }
        other => panic!("Expected PackageManager kind for pnpm, found {:?}", other),
    }
    assert!(
        !pnpm_req.additional_evidence.is_empty(),
        "Consolidated pnpm requirement must preserve additional evidence from package.json"
    );

    // Check exact pin on Java (must NOT be coerced to >=)
    let java_req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "java")
        .expect("java requirement");
    match &java_req.kind {
        concord_core::ir::RequirementKind::Runtime { name, constraint } => {
            assert_eq!(name, "java");
            assert_eq!(
                constraint,
                &concord_core::version::VersionConstraint::Exact("21.0.2".to_string())
            );
        }
        other => panic!("Expected Runtime kind for java, found {:?}", other),
    }

    // Check classification and scopes
    let terragrunt = manifest
        .requirements
        .iter()
        .find(|r| r.name == "terragrunt")
        .expect("terragrunt");
    assert!(matches!(
        &terragrunt.kind,
        concord_core::ir::RequirementKind::DeveloperTool {
            scope: concord_core::ir::ToolScope::RequiredForTask,
            ..
        }
    ));

    let opentofu = manifest
        .requirements
        .iter()
        .find(|r| r.name == "opentofu")
        .expect("opentofu");
    assert!(matches!(
        &opentofu.kind,
        concord_core::ir::RequirementKind::DeveloperTool {
            scope: concord_core::ir::ToolScope::RequiredForTask,
            ..
        }
    ));

    let openapi = manifest
        .requirements
        .iter()
        .find(|r| r.name.contains("openapi-generator-cli"))
        .expect("openapi-generator-cli");
    assert!(matches!(
        &openapi.kind,
        concord_core::ir::RequirementKind::CodeGenerator {
            scope: concord_core::ir::ToolScope::RequiredForTask,
            ..
        }
    ));

    let oazapfts = manifest
        .requirements
        .iter()
        .find(|r| r.name.contains("oazapfts"))
        .expect("oazapfts");
    assert!(matches!(
        &oazapfts.kind,
        concord_core::ir::RequirementKind::CodeGenerator { .. }
    ));

    let extism = manifest
        .requirements
        .iter()
        .find(|r| r.name.contains("extism"))
        .expect("extism");
    assert!(matches!(
        &extism.kind,
        concord_core::ir::RequirementKind::DeveloperTool { .. }
    ));

    // Evaluate predictions against empty machine
    let machine = concord_core::ir::MachineCapability {
        os: "Linux".to_string(),
        os_family: "linux".to_string(),
        arch: "x86_64".to_string(),
        cpu_count: 8,
        total_memory_bytes: 16 * 1024 * 1024 * 1024,
        available_memory_bytes: 8 * 1024 * 1024 * 1024,
        runtimes: vec![],
        package_managers: vec![],
        tools: vec![],
        services: vec![],
        containers: vec![],
        listening_ports: vec![],
        env_vars: std::collections::HashMap::new(),
        path_entries: vec![],
        evidence: vec![],
    };

    let evaluated = evaluate_all(&manifest.requirements, &machine);
    let env_model = EnvironmentModel::new(manifest, machine);
    let predictions = predict_failures(&env_model, &evaluated);

    // Verify task tool prediction confidence is calibrated to Medium, not claiming application startup failure
    let tg_pred = predictions
        .iter()
        .find(|p| p.title.contains("terragrunt"))
        .expect("terragrunt prediction");
    assert_eq!(tg_pred.confidence, Confidence::Medium);
    assert!(!tg_pred.summary.contains("Application startup"));
}

#[test]
fn test_fixture_exact_runtime_pin() {
    let fixture_path = fixtures_dir().join("exact-runtime-pin");
    let manifest = analyze_project(&fixture_path).expect("analyze exact-runtime-pin");

    let java_req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "java")
        .expect("java");
    assert_eq!(
        java_req.kind,
        concord_core::ir::RequirementKind::Runtime {
            name: "java".to_string(),
            constraint: concord_core::version::VersionConstraint::Exact("21.0.2".to_string()),
        }
    );

    // Simulate Fedora / RHEL machine with Java 26.0.2.1 installed
    let machine = concord_core::ir::MachineCapability {
        os: "Linux".to_string(),
        os_family: "linux".to_string(),
        arch: "x86_64".to_string(),
        cpu_count: 8,
        total_memory_bytes: 16 * 1024 * 1024 * 1024,
        available_memory_bytes: 8 * 1024 * 1024 * 1024,
        runtimes: vec![concord_core::ir::Runtime {
            name: "java".to_string(),
            version: "26.0.2.1".to_string(),
            executable_path: PathBuf::from("/usr/bin/java"),
            evidence: concord_core::evidence::Evidence::from_executable(
                PathBuf::from("/usr/bin/java"),
                "openjdk 26.0.2.1",
                "java -version",
            ),
        }],
        package_managers: vec![],
        tools: vec![],
        services: vec![],
        containers: vec![],
        listening_ports: vec![],
        env_vars: std::collections::HashMap::new(),
        path_entries: vec![],
        evidence: vec![],
    };

    let evaluated = evaluate_all(&manifest.requirements, &machine);
    assert_eq!(evaluated.len(), 1);
    assert!(
        evaluated[0].is_violated(),
        "Java 26.0.2.1 must NOT satisfy exact pin == 21.0.2"
    );
}

#[test]
fn test_fixture_duplicate_runtime_sources() {
    let fixture_path = fixtures_dir().join("duplicate-runtime-sources");
    let manifest = analyze_project(&fixture_path).expect("analyze duplicate-runtime-sources");

    let node_reqs: Vec<_> = manifest
        .requirements
        .iter()
        .filter(|r| r.name == "node")
        .collect();
    assert_eq!(
        node_reqs.len(),
        1,
        "Duplicate node requirements across package.json and mise.toml must be consolidated"
    );

    let node_req = node_reqs[0];
    assert_eq!(
        node_req.kind,
        concord_core::ir::RequirementKind::Runtime {
            name: "node".to_string(),
            constraint: concord_core::version::VersionConstraint::Exact("24.21.0".to_string()),
        }
    );
    assert!(
        !node_req.additional_evidence.is_empty(),
        "Consolidated requirement must preserve package.json evidence"
    );
}

#[test]
fn test_fixture_a_host_postgres_requirement() {
    let fixture_path = fixtures_dir().join("host-postgres-app");
    let manifest = analyze_project(&fixture_path).expect("analyze host-postgres-app");
    assert!(manifest.compose_projects.is_empty());
    assert!(manifest.requirements.iter().any(|r| r.name == "postgresql"));

    let machine = concord_core::ir::MachineCapability::default();
    let evaluated = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let pg_eval = evaluated
        .iter()
        .find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::ServiceRunning { service, .. } if service == "postgresql"))
        .expect("Host postgresql ServiceRunning constraint");
    assert!(pg_eval.is_violated());
}

#[test]
fn test_fixture_b_compose_postgres_healthy() {
    let fixture_path = fixtures_dir().join("compose-postgres-healthy");
    let manifest = analyze_project(&fixture_path).expect("analyze compose-postgres-healthy");
    assert_eq!(manifest.compose_projects.len(), 1);

    let mut machine = concord_core::ir::MachineCapability::default();
    machine
        .containers
        .push(concord_core::ir::ContainerObservation {
            id: "c123".to_string(),
            names: vec!["compose_healthy_postgres".to_string()],
            image: "postgres:16".to_string(),
            status: concord_core::ir::ContainerStatus::Running {
                healthy: Some(true),
            },
            ports: vec![],
            compose_project: Some("compose_healthy".to_string()),
            compose_service: Some("database".to_string()),
            labels: std::collections::HashMap::new(),
            evidence: concord_core::evidence::Evidence::from_repo_file(
                std::path::PathBuf::from("docker-compose.yml"),
                None,
                "Healthy test container",
            ),
        });

    let evaluated = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let pg_eval = evaluated
        .iter()
        .find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::ComposeServiceState { service_name, .. } if service_name == "database"))
        .expect("ComposeServiceState for database");
    assert!(
        pg_eval.is_satisfied(),
        "Healthy compose container must satisfy constraint"
    );
}

#[test]
fn test_fixture_c_compose_missing_env() {
    let fixture_path = fixtures_dir().join("compose-missing-env");
    let manifest = analyze_project(&fixture_path).expect("analyze compose-missing-env");
    assert_eq!(manifest.compose_projects.len(), 1);
    assert!(!manifest.compose_projects[0].can_instantiate);

    let machine = concord_core::ir::MachineCapability::default();
    let evaluated = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let config_eval = evaluated
        .iter()
        .find(|e| {
            matches!(
                &e.constraint,
                concord_constraints::model::Constraint::ComposeConfigUnresolved { .. }
            )
        })
        .expect("ComposeConfigUnresolved constraint");
    assert!(config_eval.is_violated());

    let env_model = concord_core::ir::EnvironmentModel::new(manifest, machine);
    let predictions = concord_predictor::predict_failures(&env_model, &evaluated);
    let pred = predictions
        .iter()
        .find(|p| p.category == concord_predictor::PredictionCategory::ComposeConfigMissing)
        .expect("ComposeConfigMissing prediction");
    assert!(pred.summary.contains("docker/.env"));

    let graph = concord_graph::EnvironmentGraph::build(&env_model, &evaluated);
    let traces = graph.all_causal_traces();
    let diagnoses = concord_diagnosis::diagnose_all(&predictions, &traces);
    let diag = diagnoses
        .iter()
        .find(|d| d.root_cause.contains("missing.env_file"))
        .expect("missing env file diagnosis");
    assert!(diag.causal_chain.iter().any(|c| c.contains("docker/.env")));
}

#[test]
fn test_fixture_d_compose_resolved_env() {
    let fixture_path = fixtures_dir().join("compose-resolved-env");
    let env_file = fixture_path.join("docker").join(".env");
    std::fs::write(&env_file, "DB_PASSWORD=supersecret\n").expect("write .env for fixture d");

    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _guard = Cleanup(env_file);

    let manifest = analyze_project(&fixture_path).expect("analyze compose-resolved-env");
    assert_eq!(manifest.compose_projects.len(), 1);
    assert!(
        manifest.compose_projects[0].can_instantiate,
        "Compose project must be instantiable when .env exists"
    );

    let machine = concord_core::ir::MachineCapability::default();
    let evaluated = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    assert!(
        !evaluated.iter().any(|e| matches!(
            &e.constraint,
            concord_constraints::model::Constraint::ComposeConfigUnresolved { .. }
        )),
        "ComposeConfigUnresolved must NOT be emitted when configuration is resolved"
    );
}

#[test]
fn test_fixture_e_unrelated_postgres_container() {
    let fixture_path = fixtures_dir().join("unrelated-postgres-container");
    let manifest = analyze_project(&fixture_path).expect("analyze unrelated-postgres-container");

    // Machine has an unrelated container for project "heym" named "heym-postgres"
    let mut machine = concord_core::ir::MachineCapability::default();
    machine
        .containers
        .push(concord_core::ir::ContainerObservation {
            id: "c999".to_string(),
            names: vec!["heym-postgres".to_string()],
            image: "postgres:16".to_string(),
            status: concord_core::ir::ContainerStatus::Running {
                healthy: Some(true),
            },
            ports: vec![],
            compose_project: Some("heym".to_string()),
            compose_service: Some("postgres".to_string()),
            labels: std::collections::HashMap::new(),
            evidence: concord_core::evidence::Evidence::from_repo_file(
                std::path::PathBuf::from("docker-compose.yml"),
                None,
                "Unrelated running container",
            ),
        });

    let evaluated = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let pg_eval = evaluated
        .iter()
        .find(|e| {
            matches!(
                &e.constraint,
                concord_constraints::model::Constraint::ComposeServiceState { .. }
            )
        })
        .expect("ComposeServiceState constraint");

    if let concord_constraints::model::Constraint::ComposeServiceState { actual_state, .. } =
        &pg_eval.constraint
    {
        assert_eq!(
            actual_state, "not-created",
            "Unrelated heym-postgres container must NOT satisfy alpha_postgres"
        );
    } else {
        panic!("Expected ComposeServiceState");
    }
    assert!(pg_eval.is_violated());
}

#[test]
fn test_fixture_f_compose_stopped_container() {
    let fixture_path = fixtures_dir().join("compose-stopped-container");
    let manifest = analyze_project(&fixture_path).expect("analyze compose-stopped-container");

    // Machine has matching stopped container
    let mut machine = concord_core::ir::MachineCapability::default();
    machine
        .containers
        .push(concord_core::ir::ContainerObservation {
            id: "c888".to_string(),
            names: vec!["stopped_postgres".to_string()],
            image: "postgres:16".to_string(),
            status: concord_core::ir::ContainerStatus::Exited { exit_code: 0 },
            ports: vec![],
            compose_project: Some("stopped_proj".to_string()),
            compose_service: Some("database".to_string()),
            labels: std::collections::HashMap::new(),
            evidence: concord_core::evidence::Evidence::from_repo_file(
                std::path::PathBuf::from("docker-compose.yml"),
                None,
                "Stopped container",
            ),
        });

    let evaluated = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let pg_eval = evaluated
        .iter()
        .find(|e| {
            matches!(
                &e.constraint,
                concord_constraints::model::Constraint::ComposeServiceState { .. }
            )
        })
        .expect("ComposeServiceState constraint");
    assert!(pg_eval.is_violated());

    let env_model = concord_core::ir::EnvironmentModel::new(manifest, machine);
    let predictions = concord_predictor::predict_failures(&env_model, &evaluated);
    let pred = predictions
        .iter()
        .find(|p| p.category == concord_predictor::PredictionCategory::ContainerStopped)
        .expect("ContainerStopped prediction");
    assert!(pred.summary.contains("exited (0)"));
}

#[test]
fn test_fixture_g_multi_component_java_attribution() {
    let fixture_path = fixtures_dir().join("multi-component-java-pin");
    let manifest = analyze_project(&fixture_path).expect("analyze multi-component-java-pin");

    assert_eq!(manifest.components.len(), 2);
    let web_comp = manifest
        .components
        .iter()
        .find(|c| c.name == "web")
        .expect("web");
    let mobile_comp = manifest
        .components
        .iter()
        .find(|c| c.name == "mobile")
        .expect("mobile");

    assert!(web_comp.languages.iter().any(|l| l.contains("javascript")));
    assert!(mobile_comp.requirements.iter().any(|r| r.name == "java"));

    let machine = concord_core::ir::MachineCapability::default();
    let evaluated = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let env_model = concord_core::ir::EnvironmentModel::new(manifest, machine);

    let graph = concord_graph::EnvironmentGraph::build(&env_model, &evaluated);
    let traces = graph.all_causal_traces();
    let java_trace = traces
        .iter()
        .find(|t| matches!(&t.constraint, concord_constraints::model::Constraint::RuntimeVersion { runtime, .. } if runtime == "java"))
        .expect("Java trace");

    assert_eq!(
        java_trace.requirement.as_deref(),
        Some("mobile"),
        "Java requirement must be attributed to mobile component, NOT web"
    );
    assert!(
        java_trace.causal_steps[0].contains("mobile"),
        "Causal step must name mobile component"
    );
}

#[test]
fn test_compose_shared_missing_env_deduplication() {
    let fixture_path = fixtures_dir().join("compose-shared-missing-env");
    let manifest = analyze_project(&fixture_path).expect("analyze compose-shared-missing-env");

    assert_eq!(manifest.compose_projects.len(), 1);
    let cp = &manifest.compose_projects[0];
    assert_eq!(cp.services.len(), 2);

    let machine = concord_core::ir::MachineCapability::default();
    let evaluated = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let env_model = concord_core::ir::EnvironmentModel::new(manifest, machine);
    let predictions = concord_predictor::predict_failures(&env_model, &evaluated);

    let graph = concord_graph::EnvironmentGraph::build(&env_model, &evaluated);
    let traces = graph.all_causal_traces();
    let diagnoses = concord_diagnosis::diagnose_all(&predictions, &traces);

    let compose_diags: Vec<_> = diagnoses
        .iter()
        .filter(|d| d.root_cause.contains("missing.env_file"))
        .collect();
    assert_eq!(
        compose_diags.len(),
        1,
        "Shared missing .env must produce exactly ONE deduplicated diagnosis"
    );

    let diag = compose_diags[0];
    assert_eq!(diag.problem, "Compose configuration unresolved");
    assert_eq!(diag.root_cause, "missing.env_file:docker/.env");
    assert_eq!(
        diag.affected_services,
        vec!["database".to_string(), "redis".to_string()]
    );
    assert_eq!(
        diag.configuration_template,
        Some(std::path::PathBuf::from("docker/example.env"))
    );

    assert!(diag
        .causal_chain
        .iter()
        .any(|c| c.contains("Compose file references docker/.env")));
    assert!(diag
        .causal_chain
        .iter()
        .any(|c| c.contains("docker/.env does not exist")));
    assert!(diag
        .causal_chain
        .iter()
        .any(|c| c.contains("Configuration template found: docker/example.env")));
    assert!(diag
        .causal_chain
        .iter()
        .any(|c| c.contains("Compose project cannot be instantiated")));
    assert!(diag
        .causal_chain
        .iter()
        .any(|c| c.contains("dependent services cannot be created: database, redis")));
}

#[test]
fn test_compose_env_template_detection() {
    let fixture_path = fixtures_dir().join("compose-shared-missing-env");
    let manifest = analyze_project(&fixture_path).expect("analyze compose-shared-missing-env");

    let cp = &manifest.compose_projects[0];
    assert_eq!(cp.env_templates.len(), 1);
    assert_eq!(
        cp.env_templates[0].missing_path,
        std::path::PathBuf::from("docker/.env")
    );
    assert_eq!(
        cp.env_templates[0].template_path,
        std::path::PathBuf::from("docker/example.env")
    );

    let template_evidence = manifest
        .evidence
        .iter()
        .find(|e| e.description.contains("Configuration template found"));
    assert!(
        template_evidence.is_some(),
        "Manifest evidence must record the discovered configuration template"
    );
    assert!(template_evidence
        .unwrap()
        .description
        .contains("docker/.env"));
}

#[test]
fn test_fixture_compose_project_blocker() {
    let fixture_path = fixtures_dir().join("compose-project-blocker");
    let manifest = analyze_project(&fixture_path).expect("analyze compose-project-blocker");

    assert_eq!(manifest.compose_projects.len(), 1);
    let cp = &manifest.compose_projects[0];
    assert!(!cp.can_instantiate);
    assert_eq!(
        cp.directly_affected_services,
        vec!["api".to_string(), "worker".to_string()]
    );
    assert_eq!(
        cp.transitively_blocked_services,
        vec!["frontend".to_string(), "metrics".to_string()]
    );

    assert!(
        manifest
            .bootstrap_actions
            .iter()
            .any(|b| b.description.contains("setup.sh copies")),
        "Bootstrap action from setup.sh must be recognized"
    );

    let machine = concord_core::ir::MachineCapability::default();
    let evaluated = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let env_model = concord_core::ir::EnvironmentModel::new(manifest, machine);
    let predictions = concord_predictor::predict_failures(&env_model, &evaluated);
    let graph = concord_graph::EnvironmentGraph::build(&env_model, &evaluated);
    let traces = graph.all_causal_traces();
    let diagnoses = concord_diagnosis::diagnose_all(&predictions, &traces);

    let compose_diags: Vec<_> = diagnoses
        .iter()
        .filter(|d| d.problem == "Compose configuration unresolved")
        .collect();
    assert_eq!(
        compose_diags.len(),
        1,
        "Must produce exactly one project-level compose diagnosis"
    );
    let diag = compose_diags[0];
    assert_eq!(
        diag.directly_affected_services,
        vec!["api".to_string(), "worker".to_string()]
    );
    assert_eq!(
        diag.transitively_blocked_services,
        vec!["frontend".to_string(), "metrics".to_string()]
    );
    assert!(
        diag.bootstrap_suggestions
            .iter()
            .any(|s| s.contains("setup.sh copies")),
        "Diagnosis must include bootstrap action suggestion"
    );
}

#[test]
fn test_fixture_version_build_metadata() {
    let fixture_path = fixtures_dir().join("version-build-metadata");
    let manifest = analyze_project(&fixture_path).expect("analyze version-build-metadata");

    let pm_req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "pnpm")
        .expect("pnpm requirement");

    match &pm_req.kind {
        concord_core::ir::RequirementKind::PackageManager { constraint, .. } => {
            let c = constraint.as_ref().expect("pnpm constraint");
            assert_eq!(c.to_string(), "==11.10.0");
            assert!(
                c.matches("11.10.0"),
                "Exact pin 11.10.0 must match host version 11.10.0"
            );
            assert!(
                !c.matches("11.24.0"),
                "Exact pin 11.10.0 must not match 11.24.0"
            );
        }
        _ => panic!("Expected PackageManager requirement"),
    }
}

#[test]
fn test_fixture_env_template_optional_vars() {
    let fixture_path = fixtures_dir().join("env-template-optional-vars");
    let manifest = analyze_project(&fixture_path).expect("analyze env-template-optional-vars");

    let secret_spec = manifest
        .env_var_specs
        .iter()
        .find(|s| s.name == "SECRET_KEY")
        .expect("SECRET_KEY spec");
    assert_eq!(
        secret_spec.category,
        concord_core::ir::EnvVarCategory::Required
    );

    let proxy_spec = manifest
        .env_var_specs
        .iter()
        .find(|s| s.name == "OPTIONAL_PROXY")
        .expect("OPTIONAL_PROXY spec");
    assert_eq!(
        proxy_spec.category,
        concord_core::ir::EnvVarCategory::IntentionallyEmpty
    );
    assert_eq!(proxy_spec.default_value.as_deref(), Some(""));

    let prefix_spec = manifest
        .env_var_specs
        .iter()
        .find(|s| s.name == "APP_PREFIX")
        .expect("APP_PREFIX spec");
    assert_eq!(
        prefix_spec.category,
        concord_core::ir::EnvVarCategory::IntentionallyEmpty
    );
    assert_eq!(prefix_spec.default_value.as_deref(), Some(""));

    let debug_spec = manifest
        .env_var_specs
        .iter()
        .find(|s| s.name == "DEBUG")
        .expect("DEBUG spec");
    assert_eq!(
        debug_spec.category,
        concord_core::ir::EnvVarCategory::OptionalWithDefault
    );
    assert_eq!(debug_spec.default_value.as_deref(), Some("false"));

    let req_env_count = manifest
        .requirements
        .iter()
        .filter(|r| {
            matches!(
                &r.kind,
                concord_core::ir::RequirementKind::EnvVar { required: true, .. }
            )
        })
        .count();
    assert_eq!(
        req_env_count, 1,
        "Only SECRET_KEY should be a required env var constraint"
    );
}

#[test]
fn test_fixture_bootstrap_copy_template() {
    let fixture_path = fixtures_dir().join("bootstrap-copy-template");
    let manifest = analyze_project(&fixture_path).expect("analyze bootstrap-copy-template");

    assert!(
        manifest.bootstrap_actions.iter().any(|b| b
            .description
            .contains("Makefile copies .env.example to .env")),
        "Bootstrap action from Makefile must be recognized"
    );
}

#[test]
fn test_fixture_build_system_compiler() {
    let fixture_path = fixtures_dir().join("fixture-build-system-compiler");
    let manifest = analyze_project(&fixture_path).expect("analyze fixture-build-system-compiler");

    assert!(manifest.languages.contains(&"c".to_string()));
    assert!(manifest.requirements.iter().any(|r| r.name == "meson"));
    assert!(manifest.requirements.iter().any(|r| r.name == "ninja"));
    assert!(manifest.requirements.iter().any(|r| r.name == "c"));

    let machine = concord_scanner::scan_machine();
    let evals = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    // C compiler should be satisfied on a host with gcc or clang installed
    let compiler_eval = evals
        .iter()
        .find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::CompilerAvailable { language, .. } if language == "c"))
        .expect("compiler eval");
    assert_eq!(
        compiler_eval.status,
        concord_constraints::model::ConstraintStatus::Satisfied
    );
}

#[test]
fn test_fixture_build_system_python() {
    let fixture_path = fixtures_dir().join("fixture-build-system-python");
    let manifest = analyze_project(&fixture_path).expect("analyze fixture-build-system-python");

    assert!(manifest.requirements.iter().any(|r| matches!(
        &r.kind,
        concord_core::ir::RequirementKind::LanguagePackage { package, scope, .. }
            if package == "nonexistent_build_module" && *scope == concord_core::ir::ToolScope::RequiredForBuild
    )));

    let machine = concord_scanner::scan_machine();
    let evals = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let pkg_eval = evals
        .iter()
        .find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::LanguagePackageAvailable { package, .. } if package == "nonexistent_build_module"))
        .expect("pkg eval");
    assert!(matches!(
        pkg_eval.status,
        concord_constraints::model::ConstraintStatus::Violated { .. }
    ));

    let model = concord_core::ir::EnvironmentModel::new(manifest, machine);
    let preds = concord_predictor::predict_failures(&model, &evals);
    assert!(preds
        .iter()
        .any(|p| p.category == concord_predictor::PredictionCategory::LanguagePackageMissing));
}

#[test]
fn test_fixture_system_library() {
    let fixture_path = fixtures_dir().join("fixture-system-library");
    let manifest = analyze_project(&fixture_path).expect("analyze fixture-system-library");

    // Verify required vs optional scopes
    let req_lib = manifest
        .requirements
        .iter()
        .find(|r| r.name == "nonexistent_system_lib_xyz")
        .expect("req lib");
    assert!(
        matches!(&req_lib.kind, concord_core::ir::RequirementKind::SystemLibrary { scope, .. } if *scope == concord_core::ir::ToolScope::RequiredForBuild)
    );

    let opt_lib = manifest
        .requirements
        .iter()
        .find(|r| r.name == "some_optional_lib_abc")
        .expect("opt lib");
    assert!(
        matches!(&opt_lib.kind, concord_core::ir::RequirementKind::SystemLibrary { scope, .. } if *scope == concord_core::ir::ToolScope::Optional)
    );

    let machine = concord_scanner::scan_machine();
    let evals = concord_constraints::evaluator::evaluate_project(&manifest, &machine);

    // Required library should be violated
    let req_eval = evals.iter().find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::SystemLibraryAvailable { name, .. } if name == "nonexistent_system_lib_xyz")).expect("req eval");
    assert!(matches!(
        req_eval.status,
        concord_constraints::model::ConstraintStatus::Violated { .. }
    ));

    // Optional library should be satisfied (false-positive control)
    let opt_eval = evals.iter().find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::SystemLibraryAvailable { name, .. } if name == "some_optional_lib_abc")).expect("opt eval");
    assert_eq!(
        opt_eval.status,
        concord_constraints::model::ConstraintStatus::Satisfied
    );

    let model = concord_core::ir::EnvironmentModel::new(manifest, machine);
    let preds = concord_predictor::predict_failures(&model, &evals);
    assert!(preds
        .iter()
        .any(|p| p.category == concord_predictor::PredictionCategory::SystemLibraryMissing));
    // Optional library should NOT be in predictions
    assert!(!preds
        .iter()
        .any(|p| p.summary.contains("some_optional_lib_abc")));
}

#[test]
fn test_fixture_build_tool_version() {
    let fixture_path = fixtures_dir().join("fixture-build-tool-version");
    let manifest = analyze_project(&fixture_path).expect("analyze fixture-build-tool-version");

    let meson_req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "meson")
        .expect("meson req");
    match &meson_req.kind {
        concord_core::ir::RequirementKind::BuildTool { constraint, .. } => {
            assert!(constraint.is_some());
        }
        _ => panic!("Expected BuildTool"),
    }

    let machine = concord_scanner::scan_machine();
    let evals = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let meson_eval = evals.iter().find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::ToolAvailable { name, .. } if name == "meson")).expect("meson eval");
    assert!(matches!(
        meson_eval.status,
        concord_constraints::model::ConstraintStatus::Violated { .. }
    ));
}

#[test]
fn test_fixture_cmake_version() {
    let fixture_path = fixtures_dir().join("fixture-cmake-version");
    let manifest = analyze_project(&fixture_path).expect("analyze fixture-cmake-version");

    let cmake_req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "cmake")
        .expect("cmake req");
    match &cmake_req.kind {
        concord_core::ir::RequirementKind::BuildTool {
            constraint, scope, ..
        } => {
            assert_eq!(*scope, concord_core::ir::ToolScope::RequiredForBuild);
            assert!(constraint.is_some());
            assert!(constraint.as_ref().unwrap().matches("99.0"));
            assert!(!constraint.as_ref().unwrap().matches("3.20.0"));
        }
        _ => panic!("Expected BuildTool"),
    }

    let machine = concord_scanner::scan_machine();
    let evals = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let cmake_eval = evals
        .iter()
        .find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::ToolAvailable { name, .. } if name == "cmake"))
        .expect("cmake eval");
    assert!(matches!(
        cmake_eval.status,
        concord_constraints::model::ConstraintStatus::Violated { .. }
    ));

    let model = concord_core::ir::EnvironmentModel::new(manifest, machine);
    let preds = concord_predictor::predict_failures(&model, &evals);
    assert!(preds
        .iter()
        .any(|p| p.category == concord_predictor::PredictionCategory::ToolMissing));
}

#[test]
fn test_fixture_cmake_compiler() {
    let fixture_path = fixtures_dir().join("fixture-cmake-compiler");
    let manifest = analyze_project(&fixture_path).expect("analyze fixture-cmake-compiler");

    assert!(manifest.languages.contains(&"c".to_string()));

    let c_req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "c")
        .expect("c compiler req");
    match &c_req.kind {
        concord_core::ir::RequirementKind::Compiler {
            language,
            min_standard,
            ..
        } => {
            assert_eq!(language, "c");
            assert_eq!(min_standard.as_deref(), Some("c11"));
        }
        _ => panic!("Expected Compiler"),
    }

    let machine = concord_scanner::scan_machine();
    let evals = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let c_eval = evals
        .iter()
        .find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::CompilerAvailable { language, .. } if language == "c"))
        .expect("c eval");
    // Host has GCC 15 supporting c11, so it should be satisfied
    assert_eq!(
        c_eval.status,
        concord_constraints::model::ConstraintStatus::Satisfied
    );
}

#[test]
fn test_fixture_cmake_package_required() {
    let fixture_path = fixtures_dir().join("fixture-cmake-package");
    let manifest = analyze_project(&fixture_path).expect("analyze fixture-cmake-package");

    let pkg_req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "nonexistenttestpackage")
        .expect("nonexistenttestpackage req");
    assert!(
        matches!(&pkg_req.kind, concord_core::ir::RequirementKind::SystemLibrary { scope, .. } if *scope == concord_core::ir::ToolScope::RequiredForBuild)
    );

    let machine = concord_scanner::scan_machine();
    let evals = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let pkg_eval = evals
        .iter()
        .find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::SystemLibraryAvailable { name, .. } if name == "nonexistenttestpackage"))
        .expect("pkg eval");
    assert!(matches!(
        pkg_eval.status,
        concord_constraints::model::ConstraintStatus::Violated { .. }
    ));

    let model = concord_core::ir::EnvironmentModel::new(manifest, machine);
    let preds = concord_predictor::predict_failures(&model, &evals);
    assert!(preds
        .iter()
        .any(|p| p.category == concord_predictor::PredictionCategory::SystemLibraryMissing));
}

#[test]
fn test_fixture_cmake_optional_package() {
    let fixture_path = fixtures_dir().join("fixture-cmake-optional-package");
    let manifest = analyze_project(&fixture_path).expect("analyze fixture-cmake-optional-package");

    let opt_req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "optionaltestpackage")
        .expect("optionaltestpackage req");
    assert!(
        matches!(&opt_req.kind, concord_core::ir::RequirementKind::SystemLibrary { scope, .. } if *scope == concord_core::ir::ToolScope::Optional)
    );

    let machine = concord_scanner::scan_machine();
    let evals = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let opt_eval = evals
        .iter()
        .find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::SystemLibraryAvailable { name, .. } if name == "optionaltestpackage"))
        .expect("opt eval");
    // Strict false-positive control: optional package evaluates as Satisfied
    assert_eq!(
        opt_eval.status,
        concord_constraints::model::ConstraintStatus::Satisfied
    );

    let model = concord_core::ir::EnvironmentModel::new(manifest, machine);
    let preds = concord_predictor::predict_failures(&model, &evals);
    assert!(!preds
        .iter()
        .any(|p| p.summary.contains("optionaltestpackage")));
}

#[test]
fn test_fixture_cmake_system_library() {
    let fixture_path = fixtures_dir().join("fixture-cmake-system-library");
    let manifest = analyze_project(&fixture_path).expect("analyze fixture-cmake-system-library");

    let lib_req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "nonexistent_native_lib")
        .expect("nonexistent_native_lib req");
    assert!(
        matches!(&lib_req.kind, concord_core::ir::RequirementKind::SystemLibrary { scope, .. } if *scope == concord_core::ir::ToolScope::RequiredForBuild)
    );

    let machine = concord_scanner::scan_machine();
    let evals = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let lib_eval = evals
        .iter()
        .find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::SystemLibraryAvailable { name, .. } if name == "nonexistent_native_lib"))
        .expect("lib eval");
    assert!(matches!(
        lib_eval.status,
        concord_constraints::model::ConstraintStatus::Violated { .. }
    ));

    let model = concord_core::ir::EnvironmentModel::new(manifest, machine);
    let preds = concord_predictor::predict_failures(&model, &evals);
    assert!(preds
        .iter()
        .any(|p| p.category == concord_predictor::PredictionCategory::SystemLibraryMissing));
}

#[test]
fn test_fixture_anyof_satisfied() {
    let fixture_path = fixtures_dir().join("fixture-anyof-satisfied");
    let manifest = analyze_project(&fixture_path).expect("analyze fixture-anyof-satisfied");

    let anyof_req = manifest
        .requirements
        .iter()
        .find(|r| matches!(&r.kind, concord_core::ir::RequirementKind::AnyOf { .. }))
        .expect("AnyOf requirement");

    match &anyof_req.kind {
        concord_core::ir::RequirementKind::AnyOf {
            capability,
            alternatives,
            ..
        } => {
            assert_eq!(capability, "crypto-backend");
            assert_eq!(alternatives.len(), 2);
        }
        _ => unreachable!(),
    }

    let machine = concord_scanner::scan_machine();
    let evals = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let anyof_eval = evals
        .iter()
        .find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::AnyOf { capability, .. } if capability == "crypto-backend"))
        .expect("anyof eval");

    // OpenSSL is installed on host, so AnyOf is satisfied
    assert_eq!(
        anyof_eval.status,
        concord_constraints::model::ConstraintStatus::Satisfied
    );

    let model = concord_core::ir::EnvironmentModel::new(manifest, machine);
    let preds = concord_predictor::predict_failures(&model, &evals);
    assert!(!preds
        .iter()
        .any(|p| p.category == concord_predictor::PredictionCategory::CapabilityUnsatisfied));
}

#[test]
fn test_fixture_anyof_unsatisfied() {
    let fixture_path = fixtures_dir().join("fixture-anyof-unsatisfied");
    let manifest = analyze_project(&fixture_path).expect("analyze fixture-anyof-unsatisfied");

    let anyof_req = manifest
        .requirements
        .iter()
        .find(|r| matches!(&r.kind, concord_core::ir::RequirementKind::AnyOf { .. }))
        .expect("AnyOf requirement");

    match &anyof_req.kind {
        concord_core::ir::RequirementKind::AnyOf {
            capability,
            alternatives,
            ..
        } => {
            assert_eq!(capability, "provider-backend");
            assert_eq!(alternatives.len(), 2);
        }
        _ => unreachable!(),
    }

    let machine = concord_scanner::scan_machine();
    let evals = concord_constraints::evaluator::evaluate_project(&manifest, &machine);
    let anyof_eval = evals
        .iter()
        .find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::AnyOf { capability, .. } if capability == "provider-backend"))
        .expect("anyof eval");

    assert!(matches!(
        anyof_eval.status,
        concord_constraints::model::ConstraintStatus::Violated { .. }
    ));

    let model = concord_core::ir::EnvironmentModel::new(manifest, machine);
    let preds = concord_predictor::predict_failures(&model, &evals);
    let pred = preds
        .iter()
        .find(|p| p.category == concord_predictor::PredictionCategory::CapabilityUnsatisfied)
        .expect("CapabilityUnsatisfied prediction");
    assert!(pred.summary.contains("provider-backend"));

    let graph = concord_graph::EnvironmentGraph::build(&model, &evals);
    let traces = graph.all_causal_traces();
    let diagnoses = concord_diagnosis::diagnose_all(&preds, &traces);
    let diag = diagnoses
        .iter()
        .find(|d| {
            d.root_cause
                .contains("capability.provider-backend.unsatisfied")
        })
        .expect("capability unsatisfied diagnosis");
    assert!(diag
        .causal_chain
        .iter()
        .any(|c| c.contains("provider-backend")));
}

#[test]
fn test_system_library_version_evaluation_end_to_end() {
    let temp_dir = tempfile::tempdir().unwrap();
    let pc_v1 = "Name: libmocktls\nDescription: Mock TLS v1\nVersion: 1.1.1\nLibs: -lmocktls\n";
    let pc_v3 =
        "Name: libmockcrypto\nDescription: Mock Crypto v3\nVersion: 3.0.2\nLibs: -lmockcrypto\n";
    std::fs::write(temp_dir.path().join("libmocktls.pc"), pc_v1).unwrap();
    std::fs::write(temp_dir.path().join("libmockcrypto.pc"), pc_v3).unwrap();

    let mut machine = concord_core::ir::MachineCapability::empty();
    machine.env_vars.insert(
        "PKG_CONFIG_PATH".to_string(),
        temp_dir.path().display().to_string(),
    );

    // 1. Incompatible version: libmocktls required >= 2.0.0 (installed: 1.1.1)
    let mut manifest_incomp =
        concord_core::ir::ProjectManifest::empty("test_incomp", temp_dir.path().to_path_buf());
    manifest_incomp
        .requirements
        .push(concord_core::ir::ProjectRequirement::new(
            "libmocktls",
            concord_core::ir::RequirementKind::SystemLibrary {
                name: "libmocktls".to_string(),
                header: None,
                constraint: Some(concord_core::VersionConstraint::parse(">= 2.0.0")),
                scope: concord_core::ir::ToolScope::RequiredForBuild,
            },
            concord_core::evidence::Evidence::new(
                concord_core::evidence::EvidenceSource::DirectObservation {
                    detail: "test manifest".to_string(),
                },
                concord_core::Confidence::Confirmed,
                "requires libmocktls >= 2.0.0".to_string(),
            ),
        ));

    let evals_incomp = concord_constraints::evaluator::evaluate_project(&manifest_incomp, &machine);
    assert_eq!(evals_incomp.len(), 1);
    assert!(evals_incomp[0].is_violated());
    if let concord_constraints::model::ConstraintStatus::Violated {
        root_cause_hint,
        reason,
    } = &evals_incomp[0].status
    {
        assert_eq!(root_cause_hint, "syslib.libmocktls.version_incompatible");
        assert!(reason.contains("1.1.1"));
    }

    let model_incomp =
        concord_core::ir::EnvironmentModel::new(manifest_incomp.clone(), machine.clone());
    let preds_incomp = concord_predictor::predict_failures(&model_incomp, &evals_incomp);
    assert_eq!(preds_incomp.len(), 1);
    assert_eq!(
        preds_incomp[0].category,
        concord_predictor::PredictionCategory::SystemLibraryIncompatible
    );
    assert_eq!(
        preds_incomp[0].title,
        "System library 'libmocktls' version incompatible"
    );

    let graph_incomp = concord_graph::EnvironmentGraph::build(&model_incomp, &evals_incomp);
    let traces_incomp = graph_incomp.all_causal_traces();
    let diags_incomp = concord_diagnosis::diagnose_all(&preds_incomp, &traces_incomp);
    assert_eq!(diags_incomp.len(), 1);
    assert_eq!(
        diags_incomp[0].problem,
        "System library 'libmocktls' version incompatible"
    );
    assert!(diags_incomp[0]
        .root_cause
        .contains("syslib.libmocktls.version_incompatible"));

    // 2. Compatible version: libmockcrypto required >= 2.0.0 (installed: 3.0.2)
    let mut manifest_comp =
        concord_core::ir::ProjectManifest::empty("test_comp", temp_dir.path().to_path_buf());
    manifest_comp
        .requirements
        .push(concord_core::ir::ProjectRequirement::new(
            "libmockcrypto",
            concord_core::ir::RequirementKind::SystemLibrary {
                name: "libmockcrypto".to_string(),
                header: None,
                constraint: Some(concord_core::VersionConstraint::parse(">= 2.0.0")),
                scope: concord_core::ir::ToolScope::RequiredForBuild,
            },
            concord_core::evidence::Evidence::new(
                concord_core::evidence::EvidenceSource::DirectObservation {
                    detail: "test manifest".to_string(),
                },
                concord_core::Confidence::Confirmed,
                "requires libmockcrypto >= 2.0.0".to_string(),
            ),
        ));

    let evals_comp = concord_constraints::evaluator::evaluate_project(&manifest_comp, &machine);
    assert_eq!(evals_comp.len(), 1);
    assert_eq!(
        evals_comp[0].status,
        concord_constraints::model::ConstraintStatus::Satisfied
    );

    let model_comp = concord_core::ir::EnvironmentModel::new(manifest_comp, machine.clone());
    let preds_comp = concord_predictor::predict_failures(&model_comp, &evals_comp);
    assert!(preds_comp.is_empty());

    // 3. Disjunctive AnyOf requirement: requires (libmocktls >= 2.0 OR libmockcrypto >= 2.0)
    let mut manifest_anyof =
        concord_core::ir::ProjectManifest::empty("test_anyof", temp_dir.path().to_path_buf());
    manifest_anyof
        .requirements
        .push(concord_core::ir::ProjectRequirement::new(
            "security-backend",
            concord_core::ir::RequirementKind::AnyOf {
                capability: "security-backend".to_string(),
                alternatives: vec![
                    concord_core::ir::ProjectRequirement::new(
                        "libmocktls",
                        concord_core::ir::RequirementKind::SystemLibrary {
                            name: "libmocktls".to_string(),
                            header: None,
                            constraint: Some(concord_core::VersionConstraint::parse(">= 2.0.0")),
                            scope: concord_core::ir::ToolScope::RequiredForBuild,
                        },
                        concord_core::evidence::Evidence::new(
                            concord_core::evidence::EvidenceSource::DirectObservation {
                                detail: "test".to_string(),
                            },
                            concord_core::Confidence::Confirmed,
                            "alt 1".to_string(),
                        ),
                    ),
                    concord_core::ir::ProjectRequirement::new(
                        "libmockcrypto",
                        concord_core::ir::RequirementKind::SystemLibrary {
                            name: "libmockcrypto".to_string(),
                            header: None,
                            constraint: Some(concord_core::VersionConstraint::parse(">= 2.0.0")),
                            scope: concord_core::ir::ToolScope::RequiredForBuild,
                        },
                        concord_core::evidence::Evidence::new(
                            concord_core::evidence::EvidenceSource::DirectObservation {
                                detail: "test".to_string(),
                            },
                            concord_core::Confidence::Confirmed,
                            "alt 2".to_string(),
                        ),
                    ),
                ],
                scope: concord_core::ir::ToolScope::RequiredForBuild,
            },
            concord_core::evidence::Evidence::new(
                concord_core::evidence::EvidenceSource::DirectObservation {
                    detail: "test manifest".to_string(),
                },
                concord_core::Confidence::Confirmed,
                "requires security backend".to_string(),
            ),
        ));

    let evals_anyof = concord_constraints::evaluator::evaluate_project(&manifest_anyof, &machine);
    assert_eq!(evals_anyof.len(), 1);
    assert_eq!(
        evals_anyof[0].status,
        concord_constraints::model::ConstraintStatus::Satisfied
    );
}

#[test]
fn test_universal_code_generator_capability_and_discovery() {
    let dir = tempfile::tempdir().unwrap();
    let cmake_file = r#"
cmake_minimum_required(VERSION 3.16)
project(test_codegen C)
find_program(BISON_EXECUTABLE bison REQUIRED)
find_program(YACC_EXECUTABLE NAMES byacc yacc REQUIRED)
"#;
    std::fs::write(dir.path().join("CMakeLists.txt"), cmake_file).unwrap();

    let manifest = concord_project::analyze_project(dir.path()).expect("analyze project");

    // 1. Verify requirements discovery
    let bison_req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "bison")
        .expect("bison requirement");
    assert!(matches!(
        bison_req.kind,
        concord_core::ir::RequirementKind::CodeGenerator { .. }
    ));

    let yacc_req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "yacc_executable")
        .expect("yacc_executable requirement");
    assert!(matches!(
        yacc_req.kind,
        concord_core::ir::RequirementKind::AnyOf { .. }
    ));

    // 2. Machine lacking bison and yacc alternatives
    let machine_bare = concord_core::ir::MachineCapability::empty();

    let evals_bare = concord_constraints::evaluator::evaluate_project(&manifest, &machine_bare);
    let bison_eval = evals_bare
        .iter()
        .find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::ToolAvailable { name, .. } if name == "bison"))
        .expect("bison eval");
    assert!(matches!(
        bison_eval.status,
        concord_constraints::model::ConstraintStatus::Violated { .. }
    ));

    let model_bare = concord_core::ir::EnvironmentModel::new(manifest.clone(), machine_bare);
    let preds_bare = concord_predictor::predict_failures(&model_bare, &evals_bare);
    let bison_pred = preds_bare
        .iter()
        .find(|p| p.category == concord_predictor::PredictionCategory::CodeGeneratorMissing)
        .expect("bison CodeGeneratorMissing prediction");
    assert!(bison_pred.title.contains("Code generator 'bison' missing"));
    assert!(bison_pred
        .summary
        .contains("Source generation tasks cannot proceed"));

    let diags_bare = concord_diagnosis::diagnose_all(&preds_bare, &[]);
    let bison_diag = diags_bare
        .iter()
        .find(|d| d.problem.contains("bison"))
        .expect("bison diagnosis");
    assert_eq!(bison_diag.root_cause, "codegen.bison installed");
    assert!(bison_diag
        .causal_chain
        .iter()
        .any(|s| s.contains("source generation tasks via bison cannot proceed")));

    // 3. Machine equipped with bison and byacc
    let mut machine_equipped = concord_core::ir::MachineCapability::empty();
    machine_equipped.tools = vec![
        concord_core::ir::ToolObservation {
            name: "bison".to_string(),
            kind: concord_core::ir::ToolKind::CodeGenerator,
            version: Some("3.8.2".to_string()),
            executable_path: std::path::PathBuf::from("/usr/bin/bison"),
            evidence: concord_core::evidence::Evidence::new(
                concord_core::evidence::EvidenceSource::ExecutableInspection {
                    path: std::path::PathBuf::from("/usr/bin/bison"),
                    version_string: "3.8.2".to_string(),
                    exit_code: 0,
                },
                concord_core::Confidence::Confirmed,
                "bison 3.8.2",
            ),
        },
        concord_core::ir::ToolObservation {
            name: "byacc".to_string(),
            kind: concord_core::ir::ToolKind::CodeGenerator,
            version: Some("20210802".to_string()),
            executable_path: std::path::PathBuf::from("/usr/bin/byacc"),
            evidence: concord_core::evidence::Evidence::new(
                concord_core::evidence::EvidenceSource::ExecutableInspection {
                    path: std::path::PathBuf::from("/usr/bin/byacc"),
                    version_string: "20210802".to_string(),
                    exit_code: 0,
                },
                concord_core::Confidence::Confirmed,
                "byacc 20210802",
            ),
        },
    ];

    let evals_equipped =
        concord_constraints::evaluator::evaluate_project(&manifest, &machine_equipped);
    let bison_eval_eq = evals_equipped
        .iter()
        .find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::ToolAvailable { name, .. } if name == "bison"))
        .expect("bison eval");
    assert_eq!(
        bison_eval_eq.status,
        concord_constraints::model::ConstraintStatus::Satisfied
    );

    let yacc_eval_eq = evals_equipped
        .iter()
        .find(|e| matches!(&e.constraint, concord_constraints::model::Constraint::AnyOf { capability, .. } if capability == "yacc_executable"))
        .expect("yacc AnyOf eval");
    assert_eq!(
        yacc_eval_eq.status,
        concord_constraints::model::ConstraintStatus::Satisfied
    );
}

#[test]
fn test_universal_build_system_generation_and_bootstrap_preconditions() {
    let dir = tempfile::tempdir().unwrap();
    // 1. Setup repository with input declaration (configure.ac) and bootstrap script (autogen.sh)
    //    Initially, generated artifact 'configure' is absent.
    std::fs::write(
        dir.path().join("configure.ac"),
        "AC_INIT([test_project], [1.0])\nAC_OUTPUT\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("autogen.sh"), "#!/bin/sh\nautoreconf -fi\n").unwrap();

    let manifest = concord_project::analyze_project(dir.path()).expect("analyze project");

    // Case 4: Bootstrap action produces the artifact
    assert!(manifest
        .bootstrap_actions
        .iter()
        .any(|b| b.target_file == std::path::Path::new("configure")));
    assert_eq!(manifest.build_system_generations.len(), 1);
    let gen = &manifest.build_system_generations[0];
    assert_eq!(gen.generator_tool, "autoreconf");
    assert_eq!(
        gen.generated_artifact,
        std::path::PathBuf::from("configure")
    );
    assert_eq!(gen.downstream_build_system, "configure");
    assert_eq!(
        gen.bootstrap_script,
        Some(std::path::PathBuf::from("autogen.sh"))
    );

    // Case 3: Generated artifact absent and generator unavailable
    let machine_bare = concord_core::ir::MachineCapability::empty();
    let evals_bare = concord_constraints::evaluator::evaluate_project(&manifest, &machine_bare);
    let eval_bare = evals_bare
        .iter()
        .find(|e| {
            matches!(
                &e.constraint,
                concord_constraints::model::Constraint::BuildSystemGenerated { .. }
            )
        })
        .expect("BuildSystemGenerated eval");
    assert!(matches!(
        eval_bare.status,
        concord_constraints::model::ConstraintStatus::Violated { ref root_cause_hint, .. }
            if root_cause_hint.starts_with("build_system.generator_missing")
    ));

    // Case 6: Missing generator produces an appropriate prediction
    let model_bare =
        concord_core::ir::EnvironmentModel::new(manifest.clone(), machine_bare.clone());
    let preds_bare = concord_predictor::predict_failures(&model_bare, &evals_bare);
    let pred_bare = preds_bare
        .iter()
        .find(|p| p.category == concord_predictor::PredictionCategory::BuildSystemGeneratorMissing)
        .expect("BuildSystemGeneratorMissing prediction");
    assert!(pred_bare
        .title
        .contains("Build system generator 'autoreconf' missing"));
    assert!(pred_bare
        .summary
        .contains("requires generator 'autoreconf'"));

    // Case 5: Generator requirement appears in the causal chain
    let graph_bare = concord_graph::EnvironmentGraph::build(&model_bare, &evals_bare);
    let violations_bare = graph_bare.find_violations();
    assert_eq!(violations_bare.len(), 1);
    let trace_bare = graph_bare
        .trace_causal_chain(violations_bare[0])
        .expect("causal trace");
    assert!(trace_bare
        .root_cause
        .as_ref()
        .unwrap()
        .contains("build_system.generator.autoreconf installed"));
    assert!(trace_bare
        .causal_steps
        .iter()
        .any(|s| s.contains("generator 'autoreconf'")));

    let diags_bare = concord_diagnosis::diagnose_all(&preds_bare, &[trace_bare]);
    assert_eq!(diags_bare.len(), 1);
    assert!(diags_bare[0]
        .root_cause
        .contains("build_system.generator.autoreconf installed"));
    assert!(diags_bare[0]
        .causal_chain
        .iter()
        .any(|s| s.contains("generator 'autoreconf'")));

    // Case 2: Generated artifact absent but generator available
    let mut machine_with_gen = concord_core::ir::MachineCapability::empty();
    machine_with_gen
        .tools
        .push(concord_core::ir::ToolObservation {
            name: "autoreconf".to_string(),
            kind: concord_core::ir::ToolKind::CodeGenerator,
            version: Some("2.71".to_string()),
            executable_path: std::path::PathBuf::from("/usr/bin/autoreconf"),
            evidence: concord_core::evidence::Evidence::new(
                concord_core::evidence::EvidenceSource::ExecutableInspection {
                    path: std::path::PathBuf::from("/usr/bin/autoreconf"),
                    version_string: "2.71".to_string(),
                    exit_code: 0,
                },
                concord_core::Confidence::Confirmed,
                "autoreconf 2.71",
            ),
        });

    let evals_gen = concord_constraints::evaluator::evaluate_project(&manifest, &machine_with_gen);
    let eval_gen = evals_gen
        .iter()
        .find(|e| {
            matches!(
                &e.constraint,
                concord_constraints::model::Constraint::BuildSystemGenerated { .. }
            )
        })
        .expect("BuildSystemGenerated eval");
    assert!(matches!(
        eval_gen.status,
        concord_constraints::model::ConstraintStatus::Violated { ref root_cause_hint, .. }
            if root_cause_hint.starts_with("build_system.bootstrap_required")
    ));

    let model_gen =
        concord_core::ir::EnvironmentModel::new(manifest.clone(), machine_with_gen.clone());
    let preds_gen = concord_predictor::predict_failures(&model_gen, &evals_gen);
    let pred_gen = preds_gen
        .iter()
        .find(|p| {
            p.category == concord_predictor::PredictionCategory::BuildSystemGenerationRequired
        })
        .expect("BuildSystemGenerationRequired prediction");
    assert!(pred_gen.title.contains("generation required"));
    assert!(pred_gen.summary.contains("run the bootstrap step"));

    let graph_gen = concord_graph::EnvironmentGraph::build(&model_gen, &evals_gen);
    let trace_gen = graph_gen
        .trace_causal_chain(graph_gen.find_violations()[0])
        .expect("trace gen");
    assert!(trace_gen
        .root_cause
        .as_ref()
        .unwrap()
        .contains("build_system.configure.generated"));
    assert!(trace_gen
        .causal_steps
        .iter()
        .any(|s| s.contains("run bootstrap step before build")));

    let diags_gen = concord_diagnosis::diagnose_all(&preds_gen, &[trace_gen]);
    assert_eq!(diags_gen.len(), 1);
    assert_eq!(diags_gen[0].root_cause, "build_system.configure.generated");
    assert!(diags_gen[0]
        .bootstrap_suggestions
        .iter()
        .any(|s| s.contains("autogen.sh")));

    // Case 1 & Case 7: Generated build artifact exists & does not create a false failure
    // Even if machine lacks generator!
    std::fs::write(dir.path().join("configure"), "#!/bin/sh\necho configured\n").unwrap();
    let manifest_with_artifact =
        concord_project::analyze_project(dir.path()).expect("analyze project with artifact");

    let evals_with_artifact =
        concord_constraints::evaluator::evaluate_project(&manifest_with_artifact, &machine_bare);
    let eval_present = evals_with_artifact
        .iter()
        .find(|e| {
            matches!(
                &e.constraint,
                concord_constraints::model::Constraint::BuildSystemGenerated { .. }
            )
        })
        .expect("BuildSystemGenerated eval");
    assert_eq!(
        eval_present.status,
        concord_constraints::model::ConstraintStatus::Satisfied
    );

    let model_present =
        concord_core::ir::EnvironmentModel::new(manifest_with_artifact, machine_bare);
    let preds_present = concord_predictor::predict_failures(&model_present, &evals_with_artifact);
    assert!(
        preds_present.is_empty(),
        "Existing generated artifact must not create false failure predictions"
    );
}

#[test]
fn test_anonymous_build_system_generation_fixture() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("build.spec"), "custom build spec").unwrap();
    std::fs::write(
        dir.path().join("setup.sh"),
        "#!/bin/sh\ngenerator_a build.spec -o build.mk\n",
    )
    .unwrap();

    let manifest = concord_project::analyze_project(dir.path()).expect("analyze project");
    assert_eq!(manifest.build_system_generations.len(), 1);
    let gen = &manifest.build_system_generations[0];
    assert_eq!(gen.generator_tool, "generator_a");
    assert_eq!(
        gen.input_declaration,
        std::path::PathBuf::from("build.spec")
    );
    assert_eq!(gen.generated_artifact, std::path::PathBuf::from("build.mk"));
    assert_eq!(gen.downstream_build_system, "make");

    // 1. Generator missing
    let machine_bare = concord_core::ir::MachineCapability::empty();
    let evals_bare = concord_constraints::evaluator::evaluate_project(&manifest, &machine_bare);
    let model_bare =
        concord_core::ir::EnvironmentModel::new(manifest.clone(), machine_bare.clone());
    let preds_bare = concord_predictor::predict_failures(&model_bare, &evals_bare);
    assert!(preds_bare
        .iter()
        .any(|p| p.category == concord_predictor::PredictionCategory::BuildSystemGeneratorMissing));

    // 2. Generator available
    let mut machine_with_gen = concord_core::ir::MachineCapability::empty();
    machine_with_gen
        .tools
        .push(concord_core::ir::ToolObservation {
            name: "generator_a".to_string(),
            kind: concord_core::ir::ToolKind::CodeGenerator,
            version: Some("1.0.0".to_string()),
            executable_path: std::path::PathBuf::from("/usr/bin/generator_a"),
            evidence: concord_core::evidence::Evidence::new(
                concord_core::evidence::EvidenceSource::ExecutableInspection {
                    path: std::path::PathBuf::from("/usr/bin/generator_a"),
                    version_string: "1.0.0".to_string(),
                    exit_code: 0,
                },
                concord_core::Confidence::Confirmed,
                "generator_a 1.0.0",
            ),
        });

    let evals_gen = concord_constraints::evaluator::evaluate_project(&manifest, &machine_with_gen);
    let model_gen = concord_core::ir::EnvironmentModel::new(manifest.clone(), machine_with_gen);
    let preds_gen = concord_predictor::predict_failures(&model_gen, &evals_gen);
    assert!(preds_gen.iter().any(|p| {
        p.category == concord_predictor::PredictionCategory::BuildSystemGenerationRequired
    }));

    // 3. Artifact generated
    std::fs::write(dir.path().join("build.mk"), "all:\n\t@echo ok\n").unwrap();
    let manifest_done = concord_project::analyze_project(dir.path()).expect("analyze project");
    let evals_done =
        concord_constraints::evaluator::evaluate_project(&manifest_done, &machine_bare);
    let model_done = concord_core::ir::EnvironmentModel::new(manifest_done, machine_bare);
    let preds_done = concord_predictor::predict_failures(&model_done, &evals_done);
    assert!(preds_done.is_empty());
}

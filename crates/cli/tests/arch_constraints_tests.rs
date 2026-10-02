use concord_constraints::evaluator::evaluate_all;
use concord_constraints::model::{Constraint, ConstraintStatus};
use concord_core::arch::{match_architecture, ArchitectureMatch};
use concord_core::evidence::Evidence;
use concord_core::ir::{
    EnvironmentModel, MachineCapability, ProjectManifest, ProjectRequirement, RequirementKind,
    ToolKind, ToolObservation, ToolScope,
};
use concord_diagnosis::diagnose_all;
use concord_graph::builder::EnvironmentGraph;
use concord_predictor::predict_failures;
use std::path::PathBuf;

fn make_test_machine(arch: &str) -> MachineCapability {
    let mut m = MachineCapability::empty();
    m.os = "Linux".to_string();
    m.os_family = "linux".to_string();
    m.arch = arch.to_string();
    m
}

fn make_test_manifest(requirements: Vec<ProjectRequirement>) -> ProjectManifest {
    ProjectManifest {
        name: "anonymous_project".to_string(),
        root_path: PathBuf::from("/workspace/anonymous_project"),
        languages: vec!["c".to_string()],
        package_managers: vec![],
        requirements,
        declared_ports: vec![],
        env_vars: vec![],
        env_var_specs: vec![],
        components: vec![],
        compose_projects: vec![],
        build_system_generations: vec![],
        bootstrap_actions: vec![],
        evidence: vec![],
        docker_used: false,
    }
}

// 1. x86_64-only requirement on x86_64
#[test]
fn test_1_x86_64_only_requirement_on_x86_64() {
    let req = ProjectRequirement::new(
        "testlib",
        RequirementKind::SystemLibrary {
            name: "testlib".to_string(),
            header: None,
            constraint: None,
            scope: ToolScope::RequiredForBuild,
        },
        Evidence::from_repo_file(PathBuf::from("build.conf"), Some(1), "testlib requirement"),
    )
    .with_arch("x86_64");

    let machine = make_test_machine("x86_64");
    let evals = evaluate_all(std::slice::from_ref(&req), &machine);

    assert_eq!(evals.len(), 1);
    assert!(
        evals[0].is_violated(),
        "x86_64-only requirement must be active and violated when missing on x86_64"
    );

    let model = EnvironmentModel {
        project: make_test_manifest(vec![req.clone()]),
        machine: machine.clone(),
    };
    let predictions = predict_failures(&model, &evals);
    assert_eq!(
        predictions.len(),
        1,
        "Active missing requirement must produce failure prediction"
    );

    // When tool/library is present on x86_64:
    let mut machine_with_lib = machine;
    machine_with_lib.tools.push(ToolObservation {
        name: "testlib".to_string(),
        kind: ToolKind::BuildTool,
        version: Some("1.0.0".to_string()),
        executable_path: PathBuf::from("/usr/bin/testlib"),
        evidence: Evidence::from_executable(
            PathBuf::from("/usr/bin/testlib"),
            "1.0.0",
            "observed testlib",
        ),
        target_triple: None,
    });
    // SystemLibrary uses search, so test with ToolAvailable to verify satisfied path
    let tool_req = ProjectRequirement::new(
        "testlib",
        RequirementKind::BuildTool {
            name: "testlib".to_string(),
            constraint: None,
            scope: ToolScope::RequiredForBuild,
        },
        Evidence::from_repo_file(PathBuf::from("build.conf"), Some(1), "testlib tool"),
    )
    .with_arch("x86_64");
    let satisfied_evals = evaluate_all(&[tool_req], &machine_with_lib);
    assert!(
        satisfied_evals[0].is_satisfied(),
        "Present requirement on matching arch must evaluate to Satisfied"
    );
}

// 2. x86_64-only requirement on aarch64
#[test]
fn test_2_x86_64_only_requirement_on_aarch64() {
    let req = ProjectRequirement::new(
        "testlib",
        RequirementKind::SystemLibrary {
            name: "testlib".to_string(),
            header: None,
            constraint: None,
            scope: ToolScope::RequiredForBuild,
        },
        Evidence::from_repo_file(PathBuf::from("build.conf"), Some(1), "testlib requirement"),
    )
    .with_arch("x86_64");

    let machine = make_test_machine("aarch64");
    let evals = evaluate_all(&[req], &machine);

    assert_eq!(evals.len(), 1);
    assert!(
        evals[0].is_not_applicable(),
        "x86_64 requirement on aarch64 host must be NotApplicable"
    );
    assert!(!evals[0].is_violated());
    assert!(!evals[0].is_satisfied());
}

// 3. aarch64-only requirement on x86_64
#[test]
fn test_3_aarch64_only_requirement_on_x86_64() {
    let req = ProjectRequirement::new(
        "testlib",
        RequirementKind::SystemLibrary {
            name: "testlib".to_string(),
            header: None,
            constraint: None,
            scope: ToolScope::RequiredForBuild,
        },
        Evidence::from_repo_file(PathBuf::from("build.conf"), Some(1), "testlib requirement"),
    )
    .with_arch("aarch64");

    let machine = make_test_machine("x86_64");
    let evals = evaluate_all(&[req], &machine);

    assert_eq!(evals.len(), 1);
    assert!(
        evals[0].is_not_applicable(),
        "aarch64 requirement on x86_64 host must be NotApplicable"
    );
    assert!(!evals[0].is_violated());
}

// 4. architecture-neutral requirement
#[test]
fn test_4_architecture_neutral_requirement() {
    let req = ProjectRequirement::new(
        "testlib",
        RequirementKind::SystemLibrary {
            name: "testlib".to_string(),
            header: None,
            constraint: None,
            scope: ToolScope::RequiredForBuild,
        },
        Evidence::from_repo_file(PathBuf::from("build.conf"), Some(1), "testlib requirement"),
    ); // no with_arch: architecture-neutral

    let machine_x86 = make_test_machine("x86_64");
    let evals_x86 = evaluate_all(std::slice::from_ref(&req), &machine_x86);
    assert!(
        evals_x86[0].is_violated(),
        "Neutral requirement must be active on x86_64"
    );

    let machine_arm = make_test_machine("aarch64");
    let evals_arm = evaluate_all(&[req], &machine_arm);
    assert!(
        evals_arm[0].is_violated(),
        "Neutral requirement must be active on aarch64"
    );
}

// 5. architecture alias normalization
#[test]
fn test_5_architecture_alias_normalization() {
    assert_eq!(
        match_architecture("amd64", "x86_64"),
        ArchitectureMatch::Matches
    );
    assert_eq!(
        match_architecture("x86_64", "amd64"),
        ArchitectureMatch::Matches
    );
    assert_eq!(
        match_architecture("arm64", "aarch64"),
        ArchitectureMatch::Matches
    );
    assert_eq!(
        match_architecture("aarch64", "arm64"),
        ArchitectureMatch::Matches
    );
    assert_eq!(
        match_architecture("armv7l", "armv7"),
        ArchitectureMatch::Matches
    );
    assert_eq!(
        match_architecture("armv7", "aarch64"),
        ArchitectureMatch::Mismatch,
        "Distinct ARM ISAs must not be over-collapsed"
    );

    // Test requirement with amd64 alias evaluated against x86_64 machine
    let req_amd64 = ProjectRequirement::new(
        "testlib",
        RequirementKind::SystemLibrary {
            name: "testlib".to_string(),
            header: None,
            constraint: None,
            scope: ToolScope::RequiredForBuild,
        },
        Evidence::from_repo_file(PathBuf::from("build.conf"), Some(1), "testlib requirement"),
    )
    .with_arch("amd64");

    let machine = make_test_machine("x86_64");
    let evals = evaluate_all(&[req_amd64], &machine);
    assert!(
        evals[0].is_violated(),
        "amd64 alias must match x86_64 machine capability and evaluate as active"
    );

    // Test requirement with arm64 alias evaluated against aarch64 machine
    let req_arm64 = ProjectRequirement::new(
        "testlib",
        RequirementKind::SystemLibrary {
            name: "testlib".to_string(),
            header: None,
            constraint: None,
            scope: ToolScope::RequiredForBuild,
        },
        Evidence::from_repo_file(PathBuf::from("build.conf"), Some(1), "testlib requirement"),
    )
    .with_arch("arm64");

    let machine_arm = make_test_machine("aarch64");
    let evals_arm = evaluate_all(&[req_arm64], &machine_arm);
    assert!(
        evals_arm[0].is_violated(),
        "arm64 alias must match aarch64 machine capability and evaluate as active"
    );
}

// 6. AnyOf containing architecture-scoped alternatives
#[test]
fn test_6_anyof_containing_architecture_scoped_alternatives() {
    let alt_x86 = ProjectRequirement::new(
        "provider_a",
        RequirementKind::BuildTool {
            name: "provider_a".to_string(),
            constraint: None,
            scope: ToolScope::RequiredForBuild,
        },
        Evidence::from_repo_file(PathBuf::from("build.conf"), Some(1), "provider_a on x86"),
    )
    .with_arch("x86_64");

    let alt_arm = ProjectRequirement::new(
        "provider_b",
        RequirementKind::BuildTool {
            name: "provider_b".to_string(),
            constraint: None,
            scope: ToolScope::RequiredForBuild,
        },
        Evidence::from_repo_file(PathBuf::from("build.conf"), Some(2), "provider_b on arm"),
    )
    .with_arch("aarch64");

    let anyof_req = ProjectRequirement::new(
        "crypto_backend",
        RequirementKind::AnyOf {
            capability: "crypto_backend".to_string(),
            alternatives: vec![alt_x86, alt_arm],
            scope: ToolScope::RequiredForBuild,
        },
        Evidence::from_repo_file(PathBuf::from("build.conf"), Some(1), "crypto capability"),
    );

    // On x86_64 host with neither provider:
    let machine_x86 = make_test_machine("x86_64");
    let evals_x86 = evaluate_all(std::slice::from_ref(&anyof_req), &machine_x86);
    assert_eq!(evals_x86.len(), 1);
    assert!(evals_x86[0].is_violated());
    if let ConstraintStatus::Violated { reason, .. } = &evals_x86[0].status {
        assert!(
            reason.contains("provider_a"),
            "Violation reason must mention applicable provider_a: {}",
            reason
        );
        assert!(
            !reason.contains("provider_b"),
            "Violation reason must not claim non-applicable ARM provider_b failed: {}",
            reason
        );
    }

    // On x86_64 host with provider_a installed:
    let mut machine_x86_with_a = make_test_machine("x86_64");
    machine_x86_with_a.tools.push(ToolObservation {
        name: "provider_a".to_string(),
        kind: ToolKind::BuildTool,
        version: Some("1.0.0".to_string()),
        executable_path: PathBuf::from("/usr/bin/provider_a"),
        evidence: Evidence::from_executable(
            PathBuf::from("/usr/bin/provider_a"),
            "1.0.0",
            "observed provider_a",
        ),
        target_triple: None,
    });
    let evals_x86_sat = evaluate_all(std::slice::from_ref(&anyof_req), &machine_x86_with_a);
    assert!(
        evals_x86_sat[0].is_satisfied(),
        "AnyOf must be satisfied when applicable x86_64 provider is present"
    );

    // On aarch64 host with neither provider:
    let machine_arm = make_test_machine("aarch64");
    let evals_arm = evaluate_all(std::slice::from_ref(&anyof_req), &machine_arm);
    assert_eq!(evals_arm.len(), 1);
    assert!(evals_arm[0].is_violated());
    if let ConstraintStatus::Violated { reason, .. } = &evals_arm[0].status {
        assert!(
            reason.contains("provider_b"),
            "Violation reason must mention applicable provider_b: {}",
            reason
        );
        assert!(
            !reason.contains("provider_a"),
            "Violation reason must not claim non-applicable x86 provider_a failed: {}",
            reason
        );
    }

    // On aarch64 host with provider_b installed:
    let mut machine_arm_with_b = make_test_machine("aarch64");
    machine_arm_with_b.tools.push(ToolObservation {
        name: "provider_b".to_string(),
        kind: ToolKind::BuildTool,
        version: Some("1.0.0".to_string()),
        executable_path: PathBuf::from("/usr/bin/provider_b"),
        evidence: Evidence::from_executable(
            PathBuf::from("/usr/bin/provider_b"),
            "1.0.0",
            "observed provider_b",
        ),
        target_triple: None,
    });
    let evals_arm_sat = evaluate_all(&[anyof_req], &machine_arm_with_b);
    assert!(
        evals_arm_sat[0].is_satisfied(),
        "AnyOf must be satisfied when applicable aarch64 provider is present"
    );
}

// 7. architecture mismatch must not produce a false failure
#[test]
fn test_7_architecture_mismatch_must_not_produce_false_failure() {
    let req = ProjectRequirement::new(
        "provider_a",
        RequirementKind::SystemLibrary {
            name: "provider_a".to_string(),
            header: None,
            constraint: None,
            scope: ToolScope::RequiredForBuild,
        },
        Evidence::from_repo_file(PathBuf::from("build.conf"), Some(1), "provider_a on arm"),
    )
    .with_arch("aarch64");

    let machine = make_test_machine("x86_64");
    let evals = evaluate_all(std::slice::from_ref(&req), &machine);

    let model = EnvironmentModel {
        project: make_test_manifest(vec![req]),
        machine,
    };

    let predictions = predict_failures(&model, &evals);
    assert!(
        predictions.is_empty(),
        "Mismatched architecture must produce 0 failure predictions"
    );

    let graph = EnvironmentGraph::build(&model, &evals);
    assert!(
        graph.find_violations().is_empty(),
        "Graph must have 0 violations for non-applicable architecture requirements"
    );
    assert!(
        graph.all_causal_traces().is_empty(),
        "Graph must have 0 causal failure traces for non-applicable requirements"
    );
}

// 8. unknown architecture must preserve uncertainty
#[test]
fn test_8_unknown_architecture_must_preserve_uncertainty() {
    let req = ProjectRequirement::new(
        "testlib",
        RequirementKind::SystemLibrary {
            name: "testlib".to_string(),
            header: None,
            constraint: None,
            scope: ToolScope::RequiredForBuild,
        },
        Evidence::from_repo_file(PathBuf::from("build.conf"), Some(1), "testlib custom"),
    )
    .with_arch("unknown_custom_isa");

    let machine = make_test_machine("x86_64");
    let evals = evaluate_all(std::slice::from_ref(&req), &machine);

    assert_eq!(evals.len(), 1);
    assert!(
        evals[0].is_unknown(),
        "Unrecognized architecture condition must preserve uncertainty"
    );
    assert!(!evals[0].is_violated());
    assert!(!evals[0].is_satisfied());

    let model = EnvironmentModel {
        project: make_test_manifest(vec![req]),
        machine,
    };
    let predictions = predict_failures(&model, &evals);
    assert!(
        predictions.is_empty(),
        "Unknown architecture must not generate deterministic failure predictions"
    );
}

// 9. graph/diagnosis retains architecture applicability
#[test]
fn test_9_graph_and_diagnosis_retains_architecture_applicability() {
    let req = ProjectRequirement::new(
        "testlib",
        RequirementKind::BuildTool {
            name: "testlib".to_string(),
            constraint: None,
            scope: ToolScope::RequiredForBuild,
        },
        Evidence::from_repo_file(
            PathBuf::from("build.conf"),
            Some(1),
            "testlib x86 requirement",
        ),
    )
    .with_arch("x86_64");

    let machine = make_test_machine("x86_64");
    let evals = evaluate_all(std::slice::from_ref(&req), &machine);

    let model = EnvironmentModel {
        project: make_test_manifest(vec![req]),
        machine,
    };

    let predictions = predict_failures(&model, &evals);
    assert_eq!(predictions.len(), 1);

    let graph = EnvironmentGraph::build(&model, &evals);
    let traces = graph.all_causal_traces();
    assert_eq!(traces.len(), 1);

    // Verify graph causal trace steps preserve architecture condition
    let trace_mentions_arch = traces[0]
        .causal_steps
        .iter()
        .any(|step| step.contains("architecture 'x86_64'") || step.contains("x86_64"));
    assert!(
        trace_mentions_arch,
        "Graph causal trace steps must retain architecture condition: {:?}",
        traces[0].causal_steps
    );

    // Verify diagnosis chain preserves architecture condition
    let diagnoses = diagnose_all(&predictions, &traces);
    assert_eq!(diagnoses.len(), 1);
    let diagnosis_mentions_arch = diagnoses[0]
        .causal_chain
        .iter()
        .any(|step| step.contains("architecture 'x86_64'") || step.contains("x86_64"));
    assert!(
        diagnosis_mentions_arch,
        "Diagnosis causal chain must retain architecture condition: {:?}",
        diagnoses[0].causal_chain
    );
}

// -------------------------------------------------------------------------
// Deepened Build-System Architecture Guard Extraction & Evaluation Tests
// -------------------------------------------------------------------------

#[test]
fn test_arch_guard_1_cmake_x86_64_condition() {
    let dir = tempfile::tempdir().unwrap();
    let cmake = r#"
cmake_minimum_required(VERSION 3.14)
project(test_guard_x86 C)
if(CMAKE_SYSTEM_PROCESSOR STREQUAL "x86_64")
    find_package(special_x86_pkg REQUIRED)
endif()
"#;
    std::fs::write(dir.path().join("CMakeLists.txt"), cmake).unwrap();

    let manifest = concord_project::analyze_project(dir.path()).expect("analyze cmake");
    let req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "special_x86_pkg")
        .expect("special_x86_pkg requirement");
    assert_eq!(req.arch.as_deref(), Some("x86_64"));
}

#[test]
fn test_arch_guard_2_cmake_aarch64_condition() {
    let dir = tempfile::tempdir().unwrap();
    let cmake = r#"
cmake_minimum_required(VERSION 3.14)
project(test_guard_arm C)
if(CMAKE_SYSTEM_PROCESSOR MATCHES "aarch64|arm64")
    find_package(special_arm_pkg REQUIRED)
endif()
"#;
    std::fs::write(dir.path().join("CMakeLists.txt"), cmake).unwrap();

    let manifest = concord_project::analyze_project(dir.path()).expect("analyze cmake");
    let req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "special_arm_pkg")
        .expect("special_arm_pkg requirement");
    assert_eq!(req.arch.as_deref(), Some("aarch64"));
}

#[test]
fn test_arch_guard_3_meson_x86_64_condition() {
    let dir = tempfile::tempdir().unwrap();
    let meson = r#"
project('test_guard_meson_x86', 'c')
if host_machine.cpu_family() == 'x86_64'
    dependency('special_x86_meson_dep')
endif
"#;
    std::fs::write(dir.path().join("meson.build"), meson).unwrap();

    let manifest = concord_project::analyze_project(dir.path()).expect("analyze meson");
    let req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "special_x86_meson_dep")
        .expect("special_x86_meson_dep requirement");
    assert_eq!(req.arch.as_deref(), Some("x86_64"));
}

#[test]
fn test_arch_guard_4_meson_aarch64_condition() {
    let dir = tempfile::tempdir().unwrap();
    let meson = r#"
project('test_guard_meson_arm', 'c')
if host_machine.cpu_family() == 'aarch64'
    dependency('special_arm_meson_dep')
endif
"#;
    std::fs::write(dir.path().join("meson.build"), meson).unwrap();

    let manifest = concord_project::analyze_project(dir.path()).expect("analyze meson");
    let req = manifest
        .requirements
        .iter()
        .find(|r| r.name == "special_arm_meson_dep")
        .expect("special_arm_meson_dep requirement");
    assert_eq!(req.arch.as_deref(), Some("aarch64"));
}

#[test]
fn test_arch_guard_5_architecture_mismatch_produces_no_failure() {
    let dir = tempfile::tempdir().unwrap();
    let cmake = r#"
cmake_minimum_required(VERSION 3.14)
project(test_mismatch C)
if(CMAKE_SYSTEM_PROCESSOR MATCHES "aarch64|arm64")
    find_package(unobtainable_arm_pkg REQUIRED)
endif()
"#;
    std::fs::write(dir.path().join("CMakeLists.txt"), cmake).unwrap();

    let manifest = concord_project::analyze_project(dir.path()).expect("analyze cmake");
    let machine = make_test_machine("x86_64");

    let evals = evaluate_all(&manifest.requirements, &machine);
    let arm_eval = evals
        .iter()
        .find(|e| {
            if let Constraint::EnvironmentGuarded { arch, .. } = &e.constraint {
                arch.as_deref() == Some("aarch64")
            } else {
                false
            }
        })
        .expect("arm guarded constraint");

    assert!(
        matches!(arm_eval.status, ConstraintStatus::NotApplicable { .. }),
        "ARM requirement on x86_64 must be NotApplicable"
    );

    let model = EnvironmentModel::new(manifest, machine);
    let preds = predict_failures(&model, &evals);
    assert!(
        !preds
            .iter()
            .any(|p| p.summary.contains("unobtainable_arm_pkg")),
        "Architecture mismatch must produce zero failure predictions for the mismatched requirement"
    );
}

#[test]
fn test_arch_guard_6_architecture_match_activates_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let cmake = r#"
cmake_minimum_required(VERSION 3.14)
project(test_match C)
if(CMAKE_SYSTEM_PROCESSOR STREQUAL "x86_64")
    find_package(missing_x86_pkg REQUIRED)
endif()
"#;
    std::fs::write(dir.path().join("CMakeLists.txt"), cmake).unwrap();

    let manifest = concord_project::analyze_project(dir.path()).expect("analyze cmake");
    // Empty machine capability on x86_64
    let machine = make_test_machine("x86_64");

    let evals = evaluate_all(&manifest.requirements, &machine);
    let x86_eval = evals
        .iter()
        .find(|e| {
            if let Constraint::EnvironmentGuarded { arch, .. } = &e.constraint {
                arch.as_deref() == Some("x86_64")
            } else {
                false
            }
        })
        .expect("x86 guarded constraint");

    assert!(
        x86_eval.is_violated(),
        "Matching x86_64 requirement must be activated and evaluated as Violated when missing"
    );

    let model = EnvironmentModel::new(manifest, machine);
    let preds = predict_failures(&model, &evals);
    assert!(
        preds.iter().any(|p| p.summary.contains("missing_x86_pkg")),
        "Matching architecture must predict failure for missing requirement"
    );
}

#[test]
fn test_arch_guard_7_unresolved_dynamic_architecture_remains_uncertain() {
    let dir = tempfile::tempdir().unwrap();
    let cmake = r#"
cmake_minimum_required(VERSION 3.14)
project(test_dynamic C)
if(CMAKE_SYSTEM_PROCESSOR STREQUAL DYNAMIC_TARGET_ARCH)
    find_package(dynamic_pkg REQUIRED)
endif()
"#;
    std::fs::write(dir.path().join("CMakeLists.txt"), cmake).unwrap();

    let manifest = concord_project::analyze_project(dir.path()).expect("analyze cmake");
    let machine = make_test_machine("x86_64");

    let evals = evaluate_all(&manifest.requirements, &machine);
    let dyn_eval = evals
        .iter()
        .find(|e| {
            if let Constraint::EnvironmentGuarded { arch, .. } = &e.constraint {
                arch.as_ref()
                    .map(|a| a.contains("DYNAMIC"))
                    .unwrap_or(false)
            } else {
                false
            }
        })
        .expect("dynamic guarded constraint");

    assert!(
        dyn_eval.is_unknown(),
        "Dynamic unresolved architecture condition must evaluate to Unknown status"
    );
    assert!(!dyn_eval.is_violated());
    assert!(!dyn_eval.is_satisfied());

    let model = EnvironmentModel::new(manifest, machine);
    let preds = predict_failures(&model, &evals);
    assert!(
        !preds.iter().any(|p| p.summary.contains("dynamic_pkg")),
        "Unresolved architecture condition must preserve uncertainty and produce 0 failure predictions"
    );
}

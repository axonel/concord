use concord_constraints::evaluator::evaluate_all;
use concord_constraints::model::ConstraintStatus;
use concord_core::arch::{match_compiler_target, Architecture, ArchitectureMatch};
use concord_core::evidence::Evidence;
use concord_core::ir::{
    EnvironmentModel, MachineCapability, ProjectManifest, ProjectRequirement, RequirementKind,
    ToolKind, ToolObservation,
};
use concord_predictor::{predict_failures, PredictionCategory};
use std::path::PathBuf;

fn make_test_machine(host_arch: &str) -> MachineCapability {
    let mut m = MachineCapability::empty();
    m.os = "Linux".to_string();
    m.os_family = "linux".to_string();
    m.arch = host_arch.to_string();
    m
}

fn mock_evidence(file: &str, desc: &str) -> Evidence {
    Evidence::from_repo_file(PathBuf::from(file), None, desc)
}

fn make_test_manifest(requirements: Vec<ProjectRequirement>) -> ProjectManifest {
    ProjectManifest {
        name: "compiler_target_project".to_string(),
        root_path: PathBuf::from("/workspace/compiler_target_project"),
        languages: vec!["c".to_string(), "cpp".to_string()],
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

// 1. Native compiler target matches host
#[test]
fn test_1_native_compiler_target_matches_host() {
    let mut machine = make_test_machine("x86_64");
    machine.tools.push(ToolObservation {
        name: "gcc".to_string(),
        kind: ToolKind::BuildTool,
        version: Some("13.2.0".to_string()),
        executable_path: PathBuf::from("/usr/bin/gcc"),
        evidence: Evidence::from_executable(
            PathBuf::from("/usr/bin/gcc"),
            "13.2.0",
            "gcc --version",
        ),
        target_triple: Some("x86_64-pc-linux-gnu".to_string()),
    });

    // Native requirement (target: None)
    let native_req = ProjectRequirement::new(
        "c_compiler",
        RequirementKind::Compiler {
            language: "c".to_string(),
            min_standard: None,
            constraint: None,
            target: None,
        },
        mock_evidence("build.ninja", "rule cc"),
    );

    let evals = evaluate_all(&[native_req], &machine);
    assert_eq!(evals.len(), 1);
    assert!(
        evals[0].is_satisfied(),
        "Native compiler with matching target triple must satisfy native requirement"
    );

    let model = EnvironmentModel::new(make_test_manifest(vec![]), machine);
    let preds = predict_failures(&model, &evals);
    assert!(
        preds.is_empty(),
        "Satisfied native compiler must produce no failure predictions"
    );
}

// 2. Cross-compiler target differs from host intentionally
#[test]
fn test_2_cross_compiler_target_differs_from_host_intentionally() {
    let mut machine = make_test_machine("x86_64");
    let cross_tool = ToolObservation {
        name: "aarch64-linux-gnu-gcc".to_string(),
        kind: ToolKind::BuildTool,
        version: Some("13.2.0".to_string()),
        executable_path: PathBuf::from("/usr/bin/aarch64-linux-gnu-gcc"),
        evidence: Evidence::from_executable(
            PathBuf::from("/usr/bin/aarch64-linux-gnu-gcc"),
            "13.2.0",
            "aarch64-linux-gnu-gcc --version",
        ),
        target_triple: Some("aarch64-linux-gnu".to_string()),
    };

    assert!(
        cross_tool.is_cross_compiler(&machine.arch),
        "Tool targeting aarch64 on x86_64 host must be recognized as a cross-compiler"
    );
    assert_eq!(
        cross_tool.target_arch(),
        Some(Architecture::Aarch64),
        "Target arch must resolve to Aarch64"
    );

    machine.tools.push(cross_tool);

    let cross_req = ProjectRequirement::new(
        "c_compiler_arm64",
        RequirementKind::Compiler {
            language: "c".to_string(),
            min_standard: None,
            constraint: None,
            target: Some("aarch64-linux-gnu".to_string()),
        },
        mock_evidence("cross_compile.env", "CROSS_COMPILE=aarch64-linux-gnu-"),
    );

    let evals = evaluate_all(&[cross_req], &machine);
    assert_eq!(evals.len(), 1);
    assert!(
        evals[0].is_satisfied(),
        "Cross-compiler differing from host is valid and intentional when explicitly requested"
    );

    let model = EnvironmentModel::new(make_test_manifest(vec![]), machine);
    let preds = predict_failures(&model, &evals);
    assert!(
        preds.is_empty(),
        "Matching cross-compiler must produce no failure predictions"
    );
}

// 3. Required target has matching compiler
#[test]
fn test_3_required_target_has_matching_compiler() {
    let mut machine = make_test_machine("x86_64");
    machine.tools.push(ToolObservation {
        name: "riscv64-unknown-linux-gnu-g++".to_string(),
        kind: ToolKind::BuildTool,
        version: Some("12.3.0".to_string()),
        executable_path: PathBuf::from("/usr/bin/riscv64-unknown-linux-gnu-g++"),
        evidence: Evidence::from_executable(
            PathBuf::from("/usr/bin/riscv64-unknown-linux-gnu-g++"),
            "12.3.0",
            "riscv64-unknown-linux-gnu-g++ --version",
        ),
        target_triple: Some("riscv64-unknown-linux-gnu".to_string()),
    });

    let req = ProjectRequirement::new(
        "cpp_compiler_riscv",
        RequirementKind::Compiler {
            language: "cpp".to_string(),
            min_standard: Some("c++17".to_string()),
            constraint: None,
            target: Some("riscv64-unknown-linux-gnu".to_string()),
        },
        mock_evidence(
            "CMakeLists.txt",
            "set(CMAKE_CXX_COMPILER riscv64-unknown-linux-gnu-g++)",
        ),
    );

    let evals = evaluate_all(&[req], &machine);
    assert_eq!(evals.len(), 1);
    assert!(evals[0].is_satisfied());
    assert!(evals[0].machine_evidence.is_some());
}

// 4. Required target has no matching compiler
#[test]
fn test_4_required_target_has_no_matching_compiler() {
    let mut machine = make_test_machine("x86_64");
    // Machine only has native x86_64 compiler
    machine.tools.push(ToolObservation {
        name: "gcc".to_string(),
        kind: ToolKind::BuildTool,
        version: Some("13.2.0".to_string()),
        executable_path: PathBuf::from("/usr/bin/gcc"),
        evidence: Evidence::from_executable(
            PathBuf::from("/usr/bin/gcc"),
            "13.2.0",
            "gcc --version",
        ),
        target_triple: Some("x86_64-pc-linux-gnu".to_string()),
    });

    let target_req = ProjectRequirement::new(
        "c_compiler_arm64",
        RequirementKind::Compiler {
            language: "c".to_string(),
            min_standard: None,
            constraint: None,
            target: Some("aarch64-linux-gnu".to_string()),
        },
        mock_evidence("Makefile", "CC = aarch64-linux-gnu-gcc"),
    );

    let evals = evaluate_all(std::slice::from_ref(&target_req), &machine);
    assert_eq!(evals.len(), 1);
    assert!(evals[0].is_violated());
    if let ConstraintStatus::Violated {
        reason,
        root_cause_hint,
    } = &evals[0].status
    {
        assert_eq!(root_cause_hint, "c.compiler_target_missing");
        assert!(
            reason.contains("aarch64-linux-gnu"),
            "Violation reason must mention required target: {}",
            reason
        );
    } else {
        panic!("expected Violated status");
    }

    let manifest = make_test_manifest(vec![target_req]);
    let model = EnvironmentModel::new(manifest, machine);
    let preds = predict_failures(&model, &evals);
    assert_eq!(preds.len(), 1);
    assert_eq!(
        preds[0].category,
        PredictionCategory::CompilerTargetUnsupported,
        "Target mismatch must be categorized as CompilerTargetUnsupported"
    );
    assert!(preds[0]
        .title
        .contains("missing support for target 'aarch64-linux-gnu'"));
}

// 5. Unknown target triple preserves uncertainty
#[test]
fn test_5_unknown_target_triple_preserves_uncertainty() {
    let mut machine = make_test_machine("x86_64");
    machine.tools.push(ToolObservation {
        name: "custom-dsp-gcc".to_string(),
        kind: ToolKind::BuildTool,
        version: Some("9.1.0".to_string()),
        executable_path: PathBuf::from("/opt/custom/bin/custom-dsp-gcc"),
        evidence: Evidence::from_executable(
            PathBuf::from("/opt/custom/bin/custom-dsp-gcc"),
            "9.1.0",
            "custom-dsp-gcc --version",
        ),
        target_triple: Some("dsp-custom-elf".to_string()),
    });

    // Sub-case 5a: Dynamic unresolvable target
    let dyn_req = ProjectRequirement::new(
        "c_dyn_target",
        RequirementKind::Compiler {
            language: "c".to_string(),
            min_standard: None,
            constraint: None,
            target: Some("dynamic:TARGET_TRIPLE".to_string()),
        },
        mock_evidence("CMakeLists.txt", "set(TARGET_TRIPLE ${USER_TARGET})"),
    );
    let evals_dyn = evaluate_all(&[dyn_req], &machine);
    assert_eq!(evals_dyn.len(), 1);
    assert!(
        matches!(evals_dyn[0].status, ConstraintStatus::Unknown { .. }),
        "Dynamic target expression must preserve uncertainty as Unknown"
    );

    // Sub-case 5b: Unrecognized custom target triple
    let custom_req = ProjectRequirement::new(
        "c_custom_target",
        RequirementKind::Compiler {
            language: "c".to_string(),
            min_standard: None,
            constraint: None,
            target: Some("other-dsp-custom-elf".to_string()),
        },
        mock_evidence("custom.mk", "TARGET=other-dsp-custom-elf"),
    );
    assert_eq!(
        match_compiler_target("other-dsp-custom-elf", "dsp-custom-elf"),
        ArchitectureMatch::Unknown
    );

    let evals_custom = evaluate_all(&[custom_req], &machine);
    assert_eq!(evals_custom.len(), 1);
    assert!(
        matches!(evals_custom[0].status, ConstraintStatus::Unknown { .. }),
        "Unresolvable target comparison must preserve uncertainty as Unknown"
    );
}

// 6. Architecture constraints and compiler target interact correctly
#[test]
fn test_6_architecture_constraints_and_compiler_target_interact_correctly() {
    let mut machine_x86 = make_test_machine("x86_64");
    machine_x86.tools.push(ToolObservation {
        name: "gcc".to_string(),
        kind: ToolKind::BuildTool,
        version: Some("13.2.0".to_string()),
        executable_path: PathBuf::from("/usr/bin/gcc"),
        evidence: Evidence::from_executable(
            PathBuf::from("/usr/bin/gcc"),
            "13.2.0",
            "gcc --version",
        ),
        target_triple: Some("x86_64-linux-gnu".to_string()),
    });

    // Req 1: Host arch aarch64 guarded requirement
    let arm_guarded_req = ProjectRequirement::new(
        "arm_build_compiler",
        RequirementKind::Compiler {
            language: "c".to_string(),
            min_standard: None,
            constraint: None,
            target: Some("aarch64-linux-gnu".to_string()),
        },
        mock_evidence("meson.build", "if host_machine.cpu_family() == 'aarch64'"),
    )
    .with_arch("aarch64");

    // Req 2: Host arch x86_64 guarded requirement
    let x86_guarded_req = ProjectRequirement::new(
        "x86_build_compiler",
        RequirementKind::Compiler {
            language: "c".to_string(),
            min_standard: None,
            constraint: None,
            target: Some("x86_64-linux-gnu".to_string()),
        },
        mock_evidence("meson.build", "if host_machine.cpu_family() == 'x86_64'"),
    )
    .with_arch("x86_64");

    let evals = evaluate_all(&[arm_guarded_req, x86_guarded_req], &machine_x86);
    assert_eq!(evals.len(), 2);

    // ARM guarded requirement must be NotApplicable on x86_64 host (not causing false violation)
    assert!(
        matches!(evals[0].status, ConstraintStatus::NotApplicable { .. }),
        "Host architecture guard on aarch64 must render compiler requirement NotApplicable on x86_64"
    );

    // x86_64 guarded requirement must be Satisfied
    assert!(
        evals[1].is_satisfied(),
        "Host architecture guard on x86_64 must activate and be satisfied by x86_64 compiler"
    );
}

// 7. Native build cannot be satisfied by pure cross compiler
#[test]
fn test_7_native_build_cannot_be_satisfied_by_pure_cross_compiler() {
    let mut machine = make_test_machine("x86_64");
    // Only a cross compiler is available
    machine.tools.push(ToolObservation {
        name: "aarch64-linux-gnu-gcc".to_string(),
        kind: ToolKind::BuildTool,
        version: Some("13.2.0".to_string()),
        executable_path: PathBuf::from("/usr/bin/aarch64-linux-gnu-gcc"),
        evidence: Evidence::from_executable(
            PathBuf::from("/usr/bin/aarch64-linux-gnu-gcc"),
            "13.2.0",
            "aarch64-linux-gnu-gcc --version",
        ),
        target_triple: Some("aarch64-linux-gnu".to_string()),
    });

    let native_req = ProjectRequirement::new(
        "c_compiler_native",
        RequirementKind::Compiler {
            language: "c".to_string(),
            min_standard: None,
            constraint: None,
            target: None,
        },
        mock_evidence("CMakeLists.txt", "project(myapp C)"),
    );

    let evals = evaluate_all(&[native_req], &machine);
    assert_eq!(evals.len(), 1);
    assert!(
        evals[0].is_violated(),
        "Native requirement must be violated when only foreign cross-compilers exist"
    );
    if let ConstraintStatus::Violated {
        reason,
        root_cause_hint,
    } = &evals[0].status
    {
        assert_eq!(root_cause_hint, "c.compiler_missing");
        assert!(
            reason.contains("cross-compiler"),
            "Violation reason must explain that cross-compilers cannot satisfy native host: {}",
            reason
        );
    } else {
        panic!("expected Violated status");
    }
}

use concord::{execute_pipeline, ConcordReport};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

/// Validate that a serialized ConcordReport JSON adheres strictly to the contract schema.
fn assert_json_contract_validity(json: &serde_json::Value) {
    assert!(json.is_object(), "Report must be a JSON object");

    // 1. Project Manifest
    let project = json.get("project").expect("Missing 'project' field");
    assert!(project.get("name").is_some(), "Missing 'project.name'");
    assert!(
        project.get("root_path").is_some(),
        "Missing 'project.root_path'"
    );
    assert!(
        project
            .get("languages")
            .and_then(|v| v.as_array())
            .is_some(),
        "'project.languages' must be an array"
    );
    assert!(
        project
            .get("requirements")
            .and_then(|v| v.as_array())
            .is_some(),
        "'project.requirements' must be an array"
    );
    assert!(
        project.get("evidence").and_then(|v| v.as_array()).is_some(),
        "'project.evidence' must be an array"
    );

    // 2. Machine Capability
    let machine = json.get("machine").expect("Missing 'machine' field");
    assert!(
        machine
            .get("os")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "'machine.os' must be a non-empty string"
    );
    assert!(machine.get("arch").is_some(), "Missing 'machine.arch'");
    assert!(
        machine.get("runtimes").and_then(|v| v.as_array()).is_some(),
        "'machine.runtimes' must be an array"
    );
    assert!(
        machine.get("tools").and_then(|v| v.as_array()).is_some(),
        "'machine.tools' must be an array"
    );
    assert!(
        machine
            .get("package_managers")
            .and_then(|v| v.as_array())
            .is_some(),
        "'machine.package_managers' must be an array"
    );

    // 3. Evaluated Constraints
    let evals = json
        .get("evaluated_constraints")
        .and_then(|v| v.as_array())
        .expect("'evaluated_constraints' must be an array");
    for (i, eval) in evals.iter().enumerate() {
        assert!(
            eval.get("constraint").and_then(|c| c.get("type")).is_some(),
            "Evaluated constraint #{} missing 'constraint.type'",
            i
        );
        assert!(
            eval.get("status").and_then(|s| s.get("status")).is_some(),
            "Evaluated constraint #{} missing 'status.status'",
            i
        );
    }

    // 4. Predictions
    let preds = json
        .get("predictions")
        .and_then(|v| v.as_array())
        .expect("'predictions' must be an array");
    for (i, pred) in preds.iter().enumerate() {
        assert!(
            pred.get("category").is_some(),
            "Prediction #{} missing 'category'",
            i
        );
        assert!(
            pred.get("confidence").is_some(),
            "Prediction #{} missing 'confidence'",
            i
        );
        assert!(
            pred.get("summary").is_some(),
            "Prediction #{} missing 'summary'",
            i
        );
    }

    // 5. Diagnoses
    let diags = json
        .get("diagnoses")
        .and_then(|v| v.as_array())
        .expect("'diagnoses' must be an array");
    for (i, diag) in diags.iter().enumerate() {
        assert!(
            diag.get("problem").is_some(),
            "Diagnosis #{} missing 'problem'",
            i
        );
        assert!(
            diag.get("root_cause").is_some(),
            "Diagnosis #{} missing 'root_cause'",
            i
        );
        assert!(
            diag.get("causal_chain")
                .and_then(|c| c.as_array())
                .is_some(),
            "Diagnosis #{} missing 'causal_chain' array",
            i
        );
        assert!(
            diag.get("violated_constraint").is_some(),
            "Diagnosis #{} missing 'violated_constraint'",
            i
        );
        assert!(
            diag.get("affected_components")
                .and_then(|r| r.as_array())
                .is_some(),
            "Diagnosis #{} missing 'affected_components' array",
            i
        );
    }

    // 6. Verification Report
    let verif = json
        .get("verification")
        .expect("Missing 'verification' field");
    assert!(
        verif.get("success").and_then(|v| v.as_bool()).is_some(),
        "'verification.success' must be a boolean"
    );
    assert!(
        verif.get("checks").and_then(|v| v.as_array()).is_some(),
        "'verification.checks' must be an array"
    );
    assert!(
        verif.get("total_checks").and_then(|v| v.as_u64()).is_some(),
        "'verification.total_checks' must be an integer"
    );
}

#[test]
fn test_hermetic_adversarial_rust_2024_contract() {
    let dir = tempdir().unwrap();
    let cargo_toml = r#"[package]
name = "adversarial-pliron-style"
version = "0.1.0"
edition.workspace = true

[workspace.package]
edition = "2024"
"#;
    fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();

    let output = execute_pipeline(dir.path()).expect("execute_pipeline on rust 2024 fixture");
    let report = ConcordReport {
        project: output.env_model.project,
        machine: output.env_model.machine,
        evaluated_constraints: output.evaluated_constraints,
        predictions: output.predictions,
        diagnoses: output.diagnoses,
        verification: output.verification,
    };

    let json_val = serde_json::to_value(&report).expect("serialize report to JSON");
    assert_json_contract_validity(&json_val);

    // Verify edition 2024 resolution in JSON
    let evals = json_val["evaluated_constraints"].as_array().unwrap();
    let rust_eval = evals
        .iter()
        .find(|e| {
            e["constraint"]["type"] == "runtime_version" && e["constraint"]["runtime"] == "rust"
        })
        .expect("rust runtime_version constraint");
    assert_eq!(rust_eval["constraint"]["constraint"]["op"], "greater_equal");
    assert_eq!(rust_eval["constraint"]["constraint"]["version"], "1.85.0");
}

#[test]
fn test_hermetic_adversarial_cuda_cmake_contract() {
    let dir = tempdir().unwrap();
    let cmake_content = r#"cmake_minimum_required(VERSION 3.20)
project(adversarial_cuda LANGUAGES CXX)
enable_language(CUDA)
set(CMAKE_CUDA_STANDARD 17)
find_package(CUDAToolkit REQUIRED)
"#;
    fs::write(dir.path().join("CMakeLists.txt"), cmake_content).unwrap();

    let output = execute_pipeline(dir.path()).expect("execute_pipeline on cuda cmake fixture");
    let report = ConcordReport {
        project: output.env_model.project,
        machine: output.env_model.machine,
        evaluated_constraints: output.evaluated_constraints,
        predictions: output.predictions,
        diagnoses: output.diagnoses,
        verification: output.verification,
    };

    let json_val = serde_json::to_value(&report).expect("serialize report to JSON");
    assert_json_contract_validity(&json_val);

    // Verify CUDA compiler constraint exists with standard c++17
    let evals = json_val["evaluated_constraints"].as_array().unwrap();
    let cuda_eval = evals
        .iter()
        .find(|e| {
            e["constraint"]["type"] == "compiler_available" && e["constraint"]["language"] == "cuda"
        })
        .expect("cuda compiler_available constraint");
    assert_eq!(cuda_eval["constraint"]["min_standard"], "c++17");
}

#[test]
fn test_hermetic_adversarial_anyof_contract() {
    let dir = tempdir().unwrap();
    let cmake_content = r#"cmake_minimum_required(VERSION 3.10)
project(test_anyof C)
find_package(OpenSSL)
find_package(NonExistentTLS)
if(OPENSSL_FOUND)
    set(USE_CRYPTO "OpenSSL")
elseif(NONEXISTENTTLS_FOUND)
    set(USE_CRYPTO "NonExistentTLS")
else()
    message(FATAL_ERROR "No crypto provider found")
endif()
"#;
    fs::write(dir.path().join("CMakeLists.txt"), cmake_content).unwrap();

    let output = execute_pipeline(dir.path()).expect("execute_pipeline on anyof fixture");
    let report = ConcordReport {
        project: output.env_model.project,
        machine: output.env_model.machine,
        evaluated_constraints: output.evaluated_constraints,
        predictions: output.predictions,
        diagnoses: output.diagnoses,
        verification: output.verification,
    };

    let json_val = serde_json::to_value(&report).expect("serialize report to JSON");
    assert_json_contract_validity(&json_val);

    let evals = json_val["evaluated_constraints"].as_array().unwrap();
    let anyof_eval = evals
        .iter()
        .find(|e| e["constraint"]["type"] == "any_of")
        .expect("any_of constraint");
    assert_eq!(anyof_eval["constraint"]["capability"], "crypto-backend");
}

#[test]
fn test_real_world_repository_adversarial_benchmarks() {
    let supported_repos = [
        "/home/roonakyadav/Projects/concord-tests/dpdk",
        "/home/roonakyadav/Projects/concord-tests/plane",
        "/home/roonakyadav/Projects/concord-tests/immich",
        "/home/roonakyadav/Projects/concord-tests/libgit2",
        "/home/roonakyadav/Projects/cutlass",
        "/home/roonakyadav/Projects/pliron",
    ];

    let mut tested_count = 0;
    for repo_path_str in supported_repos {
        let repo_path = Path::new(repo_path_str);
        if !repo_path.exists() {
            continue;
        }

        tested_count += 1;
        let output = execute_pipeline(repo_path)
            .unwrap_or_else(|e| panic!("execute_pipeline failed on {}: {}", repo_path_str, e));

        let report = ConcordReport {
            project: output.env_model.project,
            machine: output.env_model.machine,
            evaluated_constraints: output.evaluated_constraints,
            predictions: output.predictions,
            diagnoses: output.diagnoses,
            verification: output.verification,
        };

        // Assert JSON serializability and schema invariants
        let json_str = serde_json::to_string(&report)
            .unwrap_or_else(|e| panic!("failed to serialize JSON for {}: {}", repo_path_str, e));
        let json_val: serde_json::Value = serde_json::from_str(&json_str)
            .unwrap_or_else(|e| panic!("failed to deserialize JSON for {}: {}", repo_path_str, e));

        assert_json_contract_validity(&json_val);
        assert!(
            !json_val["evaluated_constraints"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{} must discover at least one constraint",
            repo_path_str
        );
    }

    assert!(
        tested_count >= 5,
        "Must test at least 5 benchmark repos in this test environment"
    );
    println!(
        "Verified {} real-world benchmark repositories",
        tested_count
    );
}

use concord_core::evidence::{Evidence, EvidenceSource};
use concord_core::Confidence;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

/// Parse PATH into a list of existing directories.
pub fn parse_path_entries() -> Vec<PathBuf> {
    let path_var = env::var("PATH").unwrap_or_default();
    env::split_paths(&path_var).filter(|p| p.is_dir()).collect()
}

/// Collect relevant dev environment variables.
pub fn scan_env_vars() -> (HashMap<String, String>, Vec<Evidence>) {
    let relevant_keys = [
        "PATH",
        "NODE_ENV",
        "PYTHONPATH",
        "DATABASE_URL",
        "PORT",
        "DOCKER_HOST",
        "JAVA_HOME",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "BUN_INSTALL",
        "NVM_DIR",
        "PYENV_ROOT",
    ];

    let mut map = HashMap::new();
    let mut evidence = Vec::new();

    for key in relevant_keys {
        if let Ok(val) = env::var(key) {
            // Mask secrets if key looks like database password, etc.
            let display_val = if key == "DATABASE_URL" {
                "[CONFIGURED]".to_string()
            } else {
                val.clone()
            };

            evidence.push(Evidence::new(
                EvidenceSource::EnvironmentVariable {
                    key: key.to_string(),
                    value: Some(display_val.clone()),
                },
                Confidence::Confirmed,
                format!("Environment variable {} is set to {}", key, display_val),
            ));
            map.insert(key.to_string(), val);
        }
    }

    (map, evidence)
}

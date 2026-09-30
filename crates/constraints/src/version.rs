use semver::VersionReq;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionComparator {
    Exact,
    GreaterEqual,
    Greater,
    LessEqual,
    Less,
    Compatible,
}

impl fmt::Display for VersionComparator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exact => write!(f, "=="),
            Self::GreaterEqual => write!(f, ">="),
            Self::Greater => write!(f, ">"),
            Self::LessEqual => write!(f, "<="),
            Self::Less => write!(f, "<"),
            Self::Compatible => write!(f, "^"),
        }
    }
}

/// Normalize a version string like "3.11" or "20" into valid SemVer ("3.11.0" or "20.0.0").
pub fn normalize_semver(ver: &str) -> String {
    let clean = ver.trim().trim_start_matches('v').trim_start_matches('=');
    let parts: Vec<&str> = clean.split('.').collect();
    match parts.len() {
        1 => {
            if parts[0].chars().all(|c| c.is_ascii_digit()) && !parts[0].is_empty() {
                format!("{}.0.0", parts[0])
            } else {
                clean.to_string()
            }
        }
        2 => {
            if parts[0].chars().all(|c| c.is_ascii_digit())
                && parts[1].chars().all(|c| c.is_ascii_digit())
            {
                format!("{}.{}.0", parts[0], parts[1])
            } else {
                clean.to_string()
            }
        }
        _ => clean.to_string(),
    }
}

/// Parse a constraint string like ">= 3.11", "^20.0.0", "==3.12.0", "3.10" into a `semver::VersionReq`.
pub fn parse_version_req(constraint: &str) -> Option<VersionReq> {
    let trimmed = constraint.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return VersionReq::parse("*").ok();
    }

    // Try direct parse first
    if let Ok(req) = VersionReq::parse(trimmed) {
        return Some(req);
    }

    // Handle comma-separated or space-separated multiple constraints like ">=3.10, <3.13"
    let mut normalized_parts = Vec::new();
    for part in trimmed.split(',') {
        let p = part.trim();
        let (op, ver) = if let Some(rest) = p.strip_prefix(">=") {
            (">=", rest.trim())
        } else if let Some(rest) = p.strip_prefix("<=") {
            ("<=", rest.trim())
        } else if let Some(rest) = p.strip_prefix('>') {
            (">", rest.trim())
        } else if let Some(rest) = p.strip_prefix('<') {
            ("<", rest.trim())
        } else if let Some(rest) = p.strip_prefix("==") {
            ("=", rest.trim())
        } else if let Some(rest) = p.strip_prefix('=') {
            ("=", rest.trim())
        } else if let Some(rest) = p.strip_prefix('^') {
            ("^", rest.trim())
        } else if let Some(rest) = p.strip_prefix('~') {
            ("~", rest.trim())
        } else {
            ("=", p)
        };

        let norm_ver = normalize_semver(ver);
        normalized_parts.push(format!("{}{}", op, norm_ver));
    }

    let combined = normalized_parts.join(", ");
    VersionReq::parse(&combined).ok()
}

pub use concord_core::version::*;

/// Check if actual version satisfies constraint requirement.
pub fn matches_version_constraint(actual: &str, constraint: &str) -> bool {
    let parsed = VersionConstraint::parse(constraint);
    parsed.matches(actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_semver() {
        assert_eq!(normalize_semver("20"), "20.0.0");
        assert_eq!(normalize_semver("3.11"), "3.11.0");
        assert_eq!(normalize_semver("3.12.4"), "3.12.4");
        assert_eq!(normalize_semver("v1.2.3"), "1.2.3");
    }

    #[test]
    fn test_version_matching() {
        assert!(matches_version_constraint("3.12.4", ">= 3.11"));
        assert!(matches_version_constraint("3.11.0", ">= 3.11"));
        assert!(!matches_version_constraint("3.10.12", ">= 3.11"));

        assert!(matches_version_constraint("20.10.0", ">= 20"));
        assert!(matches_version_constraint("20.10.0", "^20.0.0"));
        assert!(!matches_version_constraint("18.19.0", ">= 20"));

        assert!(matches_version_constraint("3.11.5", ">=3.10, <3.13"));
        assert!(!matches_version_constraint("3.13.0", ">=3.10, <3.13"));
    }

    #[test]
    fn test_exact_pin_and_multi_component_versions() {
        // ==21.0.2 vs 21.0.2 -> satisfied
        assert!(matches_version_constraint("21.0.2", "==21.0.2"));
        assert!(matches_version_constraint("21.0.2", "21.0.2"));

        // ==21.0.2 vs 21.0.3 -> violated
        assert!(!matches_version_constraint("21.0.3", "==21.0.2"));
        assert!(!matches_version_constraint("21.0.3", "21.0.2"));

        // ==21.0.2 vs 26.0.2.1 -> violated
        assert!(!matches_version_constraint("26.0.2.1", "==21.0.2"));
        assert!(!matches_version_constraint("26.0.2.1", "21.0.2"));

        // >=21.0.2 vs 26.0.2.1 -> satisfied
        assert!(matches_version_constraint("26.0.2.1", ">=21.0.2"));

        // >=21.0.2 AND <27 -> satisfied by 26.0.2.1
        assert!(matches_version_constraint("26.0.2.1", ">=21.0.2, <27"));

        // >=21.0.2 AND <26 -> violated by 26.0.2.1
        assert!(!matches_version_constraint("26.0.2.1", ">=21.0.2, <26"));
    }
}

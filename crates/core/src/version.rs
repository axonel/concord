use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
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

/// A structured version constraint preserving exact mathematical semantics.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "op", content = "version", rename_all = "snake_case")]
pub enum VersionConstraint {
    Exact(String),
    GreaterEqual(String),
    Greater(String),
    LessEqual(String),
    Less(String),
    Compatible(String),
    Range {
        lower: (VersionComparator, String),
        upper: (VersionComparator, String),
    },
    Any,
}

impl fmt::Display for VersionConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exact(v) => write!(f, "=={}", v),
            Self::GreaterEqual(v) => write!(f, ">={}", v),
            Self::Greater(v) => write!(f, ">{}", v),
            Self::LessEqual(v) => write!(f, "<={}", v),
            Self::Less(v) => write!(f, "<{}", v),
            Self::Compatible(v) => write!(f, "^{}", v),
            Self::Range { lower, upper } => {
                write!(f, "{}{}, {}{}", lower.0, lower.1, upper.0, upper.1)
            }
            Self::Any => write!(f, "*"),
        }
    }
}

/// Separates a version string into semantic version and optional build/integrity metadata (SemVer 2.0 §10).
pub fn split_version_and_metadata(s: &str) -> (&str, Option<&str>) {
    if let Some((ver, meta)) = s.split_once('+') {
        (ver.trim(), Some(meta.trim()))
    } else {
        (s.trim(), None)
    }
}

/// Extract numeric dot/dash-separated version components from any string (e.g. "26.0.2.1", "v1.2.3").
pub fn parse_version_components(ver_str: &str) -> Vec<u64> {
    let (ver_no_meta, _) = split_version_and_metadata(ver_str);
    let clean = ver_no_meta
        .trim()
        .trim_start_matches(|c: char| !c.is_ascii_digit());

    let mut components = Vec::new();
    for part in clean.split(['.', '-', '_']) {
        let num_part: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(num) = num_part.parse::<u64>() {
            components.push(num);
        } else {
            break;
        }
    }
    components
}

/// Compares two numeric version component vectors, treating omitted trailing zeros as equivalent.
pub fn compare_version_components(a: &[u64], b: &[u64]) -> Ordering {
    let max_len = a.len().max(b.len());
    for i in 0..max_len {
        let val_a = a.get(i).copied().unwrap_or(0);
        let val_b = b.get(i).copied().unwrap_or(0);
        match val_a.cmp(&val_b) {
            Ordering::Equal => continue,
            non_eq => return non_eq,
        }
    }
    Ordering::Equal
}

impl VersionConstraint {
    /// Parse a raw constraint string into a structured `VersionConstraint`.
    /// When a version has no operator prefix (e.g. "21.0.2" or "11.24.0"), it is treated as an exact pin `==`.
    /// Build/integrity metadata (e.g. `+sha512...` or `+build.1`) is separated and ignored for version comparison.
    pub fn parse(s: &str) -> Self {
        let (ver_clean, _) = split_version_and_metadata(s);
        let trimmed = ver_clean.trim();
        if trimmed.is_empty() || trimmed == "*" {
            return Self::Any;
        }

        // Check for range like ">=21.0.2, <27" or ">=3.10 <3.13"
        if trimmed.contains(',')
            || (trimmed.contains(' ') && (trimmed.contains('<') || trimmed.contains('>')))
        {
            let parts: Vec<&str> = trimmed
                .split([',', ' '])
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .collect();

            if parts.len() == 2 {
                let parse_op_ver = |p: &str| -> Option<(VersionComparator, String)> {
                    let (clean_p, _) = split_version_and_metadata(p);
                    if let Some(rest) = clean_p.strip_prefix(">=") {
                        Some((VersionComparator::GreaterEqual, rest.trim().to_string()))
                    } else if let Some(rest) = clean_p.strip_prefix('>') {
                        Some((VersionComparator::Greater, rest.trim().to_string()))
                    } else if let Some(rest) = clean_p.strip_prefix("<=") {
                        Some((VersionComparator::LessEqual, rest.trim().to_string()))
                    } else if let Some(rest) = clean_p.strip_prefix('<') {
                        Some((VersionComparator::Less, rest.trim().to_string()))
                    } else {
                        clean_p
                            .strip_prefix("==")
                            .map(|rest| (VersionComparator::Exact, rest.trim().to_string()))
                    }
                };

                if let (Some(lower), Some(upper)) = (parse_op_ver(parts[0]), parse_op_ver(parts[1]))
                {
                    return Self::Range { lower, upper };
                }
            }
        }

        if let Some(rest) = trimmed.strip_prefix(">=") {
            Self::GreaterEqual(rest.trim().to_string())
        } else if let Some(rest) = trimmed.strip_prefix('>') {
            Self::Greater(rest.trim().to_string())
        } else if let Some(rest) = trimmed.strip_prefix("<=") {
            Self::LessEqual(rest.trim().to_string())
        } else if let Some(rest) = trimmed.strip_prefix('<') {
            Self::Less(rest.trim().to_string())
        } else if let Some(rest) = trimmed.strip_prefix("==") {
            Self::Exact(rest.trim().to_string())
        } else if let Some(rest) = trimmed.strip_prefix('=') {
            Self::Exact(rest.trim().to_string())
        } else if let Some(rest) = trimmed.strip_prefix('^') {
            Self::Compatible(rest.trim().to_string())
        } else if let Some(rest) = trimmed.strip_prefix('~') {
            Self::Compatible(rest.trim().to_string())
        } else {
            // Default with no operator is exact pin
            let clean_exact = if let Some(rest) = trimmed.strip_prefix("version_") {
                rest
            } else if let Some(rest) = trimmed.strip_prefix("version-") {
                rest
            } else if let Some(rest) = trimmed.strip_prefix('v') {
                if rest.starts_with(|c: char| c.is_ascii_digit()) {
                    rest
                } else {
                    trimmed
                }
            } else {
                trimmed
            };
            Self::Exact(clean_exact.to_string())
        }
    }

    /// Check if actual version satisfies this constraint.
    pub fn matches(&self, actual_str: &str) -> bool {
        let (actual_clean, _) = split_version_and_metadata(actual_str);
        match self {
            Self::Any => true,
            Self::Exact(expected) => {
                let actual_comp = parse_version_components(actual_clean);
                let expected_comp = parse_version_components(expected);
                if !actual_comp.is_empty() && !expected_comp.is_empty() {
                    let comps_eq =
                        compare_version_components(&actual_comp, &expected_comp) == Ordering::Equal;
                    if comps_eq {
                        if actual_clean.contains('-') || expected.contains('-') {
                            actual_clean.trim().eq_ignore_ascii_case(expected.trim())
                        } else {
                            true
                        }
                    } else {
                        false
                    }
                } else {
                    actual_clean.trim().eq_ignore_ascii_case(expected.trim())
                }
            }
            Self::GreaterEqual(expected) => {
                let actual_comp = parse_version_components(actual_clean);
                let expected_comp = parse_version_components(expected);
                if !actual_comp.is_empty() && !expected_comp.is_empty() {
                    compare_version_components(&actual_comp, &expected_comp) != Ordering::Less
                } else {
                    actual_clean.trim() >= expected.trim()
                }
            }
            Self::Greater(expected) => {
                let actual_comp = parse_version_components(actual_clean);
                let expected_comp = parse_version_components(expected);
                if !actual_comp.is_empty() && !expected_comp.is_empty() {
                    compare_version_components(&actual_comp, &expected_comp) == Ordering::Greater
                } else {
                    actual_clean.trim() > expected.trim()
                }
            }
            Self::LessEqual(expected) => {
                let actual_comp = parse_version_components(actual_clean);
                let expected_comp = parse_version_components(expected);
                if !actual_comp.is_empty() && !expected_comp.is_empty() {
                    compare_version_components(&actual_comp, &expected_comp) != Ordering::Greater
                } else {
                    actual_clean.trim() <= expected.trim()
                }
            }
            Self::Less(expected) => {
                let actual_comp = parse_version_components(actual_clean);
                let expected_comp = parse_version_components(expected);
                if !actual_comp.is_empty() && !expected_comp.is_empty() {
                    compare_version_components(&actual_comp, &expected_comp) == Ordering::Less
                } else {
                    actual_clean.trim() < expected.trim()
                }
            }
            Self::Compatible(expected) => {
                let actual_comp = parse_version_components(actual_clean);
                let expected_comp = parse_version_components(expected);
                if actual_comp.is_empty() || expected_comp.is_empty() {
                    return actual_clean.trim().starts_with(expected.trim());
                }
                let exp_major = expected_comp.first().copied().unwrap_or(0);
                let act_major = actual_comp.first().copied().unwrap_or(0);
                if exp_major != act_major {
                    return false;
                }
                compare_version_components(&actual_comp, &expected_comp) != Ordering::Less
            }
            Self::Range { lower, upper } => {
                let lower_c = match lower.0 {
                    VersionComparator::GreaterEqual => Self::GreaterEqual(lower.1.clone()),
                    VersionComparator::Greater => Self::Greater(lower.1.clone()),
                    VersionComparator::Exact => Self::Exact(lower.1.clone()),
                    _ => Self::Any,
                };
                let upper_c = match upper.0 {
                    VersionComparator::LessEqual => Self::LessEqual(upper.1.clone()),
                    VersionComparator::Less => Self::Less(upper.1.clone()),
                    VersionComparator::Exact => Self::Exact(upper.1.clone()),
                    _ => Self::Any,
                };
                lower_c.matches(actual_clean) && upper_c.matches(actual_clean)
            }
        }
    }

    /// Calculate the mathematical intersection of two constraints.
    pub fn intersect(&self, other: &Self) -> Result<Self, String> {
        match (self, other) {
            (Self::Any, o) | (o, Self::Any) => Ok(o.clone()),
            (Self::Exact(v1), Self::Exact(v2)) => {
                let c1 = parse_version_components(v1);
                let c2 = parse_version_components(v2);
                if compare_version_components(&c1, &c2) == Ordering::Equal {
                    Ok(Self::Exact(v1.clone()))
                } else {
                    Err(format!("Conflicting exact versions: =={} vs =={}", v1, v2))
                }
            }
            (Self::Exact(exact_v), Self::GreaterEqual(ge_v))
            | (Self::GreaterEqual(ge_v), Self::Exact(exact_v)) => {
                let c_exact = parse_version_components(exact_v);
                let c_ge = parse_version_components(ge_v);
                if compare_version_components(&c_exact, &c_ge) != Ordering::Less {
                    Ok(Self::Exact(exact_v.clone()))
                } else {
                    Err(format!(
                        "Exact version =={} does not satisfy minimum requirement >={}",
                        exact_v, ge_v
                    ))
                }
            }
            (Self::Compatible(compat_v), Self::Exact(exact_v))
            | (Self::Exact(exact_v), Self::Compatible(compat_v)) => {
                let c_compat = parse_version_components(compat_v);
                let c_exact = parse_version_components(exact_v);
                if !c_compat.is_empty()
                    && !c_exact.is_empty()
                    && c_compat[0] == c_exact[0]
                    && compare_version_components(&c_exact, &c_compat) != Ordering::Less
                {
                    Ok(Self::Exact(exact_v.clone()))
                } else {
                    Err(format!(
                        "Exact version =={} is incompatible with ^{}",
                        exact_v, compat_v
                    ))
                }
            }
            (Self::GreaterEqual(v1), Self::GreaterEqual(v2)) => {
                let c1 = parse_version_components(v1);
                let c2 = parse_version_components(v2);
                if compare_version_components(&c1, &c2) == Ordering::Less {
                    Ok(Self::GreaterEqual(v2.clone()))
                } else {
                    Ok(Self::GreaterEqual(v1.clone()))
                }
            }
            (Self::GreaterEqual(lower_v), Self::Less(upper_v)) => {
                let c_low = parse_version_components(lower_v);
                let c_up = parse_version_components(upper_v);
                if compare_version_components(&c_low, &c_up) == Ordering::Less {
                    Ok(Self::Range {
                        lower: (VersionComparator::GreaterEqual, lower_v.clone()),
                        upper: (VersionComparator::Less, upper_v.clone()),
                    })
                } else {
                    Err(format!(
                        "Unsatisfiable range: >={} but <{}",
                        lower_v, upper_v
                    ))
                }
            }
            _ => Ok(self.clone()),
        }
    }
}

/// Extract the first semantic version substring found in text output.
pub fn parse_first_semantic_version(output: &str) -> Option<String> {
    let mut candidate = None;
    for word in output.split_whitespace() {
        let clean = word.trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
        let parts: Vec<&str> = clean.split('.').collect();
        if parts.len() >= 2
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        {
            if parts.len() >= 3 {
                return Some(clean.to_string());
            }
            if candidate.is_none() {
                candidate = Some(clean.to_string());
            }
        }
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_pin_semantics() {
        let req = VersionConstraint::parse("21.0.2");
        assert_eq!(req, VersionConstraint::Exact("21.0.2".to_string()));

        // Exactly 21.0.2 matches 21.0.2
        assert!(req.matches("21.0.2"));
        // 21.0.3 must fail
        assert!(!req.matches("21.0.3"));
        // 26.0.2.1 must fail
        assert!(!req.matches("26.0.2.1"));
    }

    #[test]
    fn test_greater_equal_semantics() {
        let req = VersionConstraint::parse(">=21.0.2");
        assert!(req.matches("21.0.2"));
        assert!(req.matches("21.0.3"));
        assert!(req.matches("26.0.2.1"));
        assert!(!req.matches("20.0.0"));
    }

    #[test]
    fn test_range_semantics() {
        let req = VersionConstraint::parse(">=21.0.2, <27");
        assert!(req.matches("26.0.2.1"));
        assert!(req.matches("21.0.2"));
        assert!(!req.matches("27.0.0"));
        assert!(!req.matches("20.9.0"));
    }

    #[test]
    fn test_intersection_semantics() {
        let c1 = VersionConstraint::parse(">=10.0.0");
        let c2 = VersionConstraint::parse("11.24.0");
        let merged = c1.intersect(&c2).expect("intersection succeeds");
        assert_eq!(merged, VersionConstraint::Exact("11.24.0".to_string()));

        let conflict1 = VersionConstraint::parse(">=22.0.0");
        let conflict2 = VersionConstraint::parse("20.0.0");
        assert!(conflict1.intersect(&conflict2).is_err());
    }

    #[test]
    fn test_build_metadata_separation() {
        let req = VersionConstraint::parse("11.10.0+sha512.0b7f8b98060031904c017e3a41eb187a16d40eeb829b95c4f8cb03681761fc4ab53dd219115b9b447f4dce1a05a214764461e7d3703392a9f32f9511ce8c86c8");
        assert_eq!(req, VersionConstraint::Exact("11.10.0".to_string()));
        assert!(req.matches("11.10.0"));
        assert!(req.matches("11.10.0+differenthash"));
        assert!(!req.matches("11.24.0"));

        let range = VersionConstraint::parse(">=1.0.0+build.1");
        assert_eq!(range, VersionConstraint::GreaterEqual("1.0.0".to_string()));
        assert!(range.matches("1.0.0"));
        assert!(range.matches("2.0.0"));
    }

    #[test]
    fn test_prerelease_identifiers() {
        let exact_prerelease = VersionConstraint::parse("1.2.3-alpha.1");
        assert_eq!(
            exact_prerelease,
            VersionConstraint::Exact("1.2.3-alpha.1".to_string())
        );
        assert!(exact_prerelease.matches("1.2.3-alpha.1"));
        assert!(exact_prerelease.matches("1.2.3-alpha.1+sha512.abc"));
        // Prerelease must not match plain release or different prerelease
        assert!(!exact_prerelease.matches("1.2.3"));
        assert!(!exact_prerelease.matches("1.2.3-alpha.2"));

        let exact_release = VersionConstraint::parse("1.2.3");
        assert!(exact_release.matches("1.2.3"));
        assert!(!exact_release.matches("1.2.3-alpha.1"));
    }
}

use serde::{Deserialize, Serialize};
use std::fmt;

/// Canonical CPU architecture families recognized by Concord.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    X86_64,
    X86,
    Aarch64,
    Armv7,
    Armv6,
    Riscv64,
    Riscv32,
    Ppc64le,
    Ppc64,
    S390x,
    LoongArch64,
    Wasm32,
    Wasm64,
    Other(String),
}

impl Architecture {
    /// Normalize an architecture name string into a canonical Architecture enum variant.
    pub fn normalize_from_str(s: &str) -> Self {
        let clean = s.trim().to_lowercase();
        let stripped = clean.replace(['-', '_'], "");
        match clean.as_str() {
            "x86_64" | "amd64" | "x86-64" | "x64" => Self::X86_64,
            "x86" | "i386" | "i486" | "i586" | "i686" | "x86_32" | "ia32" => Self::X86,
            "aarch64" | "arm64" | "arm64e" | "armv8-a" | "armv8a" | "armv8" => Self::Aarch64,
            "armv7" | "armv7l" | "armv7a" | "armv7-a" | "armhf" | "armv7hl" => Self::Armv7,
            "armv6" | "armv6l" | "armel" => Self::Armv6,
            "riscv64" | "riscv64gc" => Self::Riscv64,
            "riscv32" => Self::Riscv32,
            "ppc64le" | "ppc64el" | "powerpc64le" => Self::Ppc64le,
            "ppc64" | "powerpc64" | "ppc" => Self::Ppc64,
            "s390x" => Self::S390x,
            "loongarch64" => Self::LoongArch64,
            "wasm32" => Self::Wasm32,
            "wasm64" => Self::Wasm64,
            _ => {
                // Secondary check without hyphens/underscores
                match stripped.as_str() {
                    "x8664" | "amd64" => Self::X86_64,
                    "aarch64" | "arm64" | "arm64e" | "armv8a" | "armv8" => Self::Aarch64,
                    "armv7" | "armv7l" | "armv7a" => Self::Armv7,
                    "armv6" | "armv6l" => Self::Armv6,
                    "riscv64" => Self::Riscv64,
                    "riscv32" => Self::Riscv32,
                    "ppc64le" | "ppc64el" => Self::Ppc64le,
                    "ppc64" => Self::Ppc64,
                    "s390x" => Self::S390x,
                    "loongarch64" => Self::LoongArch64,
                    _ => Self::Other(clean),
                }
            }
        }
    }

    /// Canonical string identifier for this architecture.
    pub fn canonical_name(&self) -> &str {
        match self {
            Self::X86_64 => "x86_64",
            Self::X86 => "x86",
            Self::Aarch64 => "aarch64",
            Self::Armv7 => "armv7",
            Self::Armv6 => "armv6",
            Self::Riscv64 => "riscv64",
            Self::Riscv32 => "riscv32",
            Self::Ppc64le => "ppc64le",
            Self::Ppc64 => "ppc64",
            Self::S390x => "s390x",
            Self::LoongArch64 => "loongarch64",
            Self::Wasm32 => "wasm32",
            Self::Wasm64 => "wasm64",
            Self::Other(name) => name.as_str(),
        }
    }

    /// Whether this architecture represents a recognized standard ISA.
    pub fn is_recognized(&self) -> bool {
        !matches!(self, Self::Other(_))
    }
}

impl fmt::Display for Architecture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.canonical_name())
    }
}

/// The outcome of matching an architecture constraint against observed machine architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchitectureMatch {
    Matches,
    Mismatch,
    Unknown,
}

/// Match an expected/required architecture against a machine architecture.
pub fn match_architecture(expected: &str, machine_arch: &str) -> ArchitectureMatch {
    let exp_trimmed = expected.trim();
    let mach_trimmed = machine_arch.trim();
    if exp_trimmed.is_empty() || mach_trimmed.is_empty() {
        return ArchitectureMatch::Unknown;
    }

    let exp_norm = Architecture::normalize_from_str(exp_trimmed);
    let mach_norm = Architecture::normalize_from_str(mach_trimmed);

    match (&exp_norm, &mach_norm) {
        (Architecture::Other(_), _) | (_, Architecture::Other(_)) => {
            if exp_trimmed.eq_ignore_ascii_case(mach_trimmed) {
                ArchitectureMatch::Matches
            } else {
                ArchitectureMatch::Unknown
            }
        }
        (a, b) => {
            if a == b {
                ArchitectureMatch::Matches
            } else {
                ArchitectureMatch::Mismatch
            }
        }
    }
}

/// Applicability state of an environment-constrained requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentApplicability {
    Applicable,
    NotApplicable,
    Unknown,
}

/// Evaluates platform and architecture constraints against host machine capabilities.
pub fn evaluate_applicability(
    platform_guard: Option<&str>,
    arch_guard: Option<&str>,
    machine_os: &str,
    machine_os_family: &str,
    machine_arch: &str,
) -> (EnvironmentApplicability, Option<String>) {
    // 1. Evaluate platform guard if specified
    if let Some(plat) = platform_guard {
        let plat_lower = plat.trim().to_lowercase();
        let os_lower = machine_os.to_lowercase();
        let family_lower = machine_os_family.to_lowercase();

        let matches_platform = match plat_lower.as_str() {
            "windows" | "win32" => os_lower.contains("windows") || family_lower.contains("windows"),
            "darwin" | "macos" | "apple" | "osx" => {
                os_lower.contains("darwin")
                    || os_lower.contains("mac")
                    || family_lower.contains("darwin")
            }
            "linux" => os_lower.contains("linux") || family_lower.contains("linux"),
            "unix" => {
                os_lower.contains("linux")
                    || os_lower.contains("darwin")
                    || os_lower.contains("bsd")
                    || family_lower.contains("unix")
            }
            "freebsd" => os_lower.contains("freebsd"),
            "openbsd" => os_lower.contains("openbsd"),
            "android" => os_lower.contains("android"),
            other => os_lower.contains(other) || family_lower.contains(other),
        };

        if !matches_platform {
            return (
                EnvironmentApplicability::NotApplicable,
                Some(format!(
                    "Requirement scoped to platform '{}', machine OS is '{}'",
                    plat, machine_os
                )),
            );
        }
    }

    // 2. Evaluate architecture guard if specified
    if let Some(arch) = arch_guard {
        match match_architecture(arch, machine_arch) {
            ArchitectureMatch::Matches => (EnvironmentApplicability::Applicable, None),
            ArchitectureMatch::Mismatch => (
                EnvironmentApplicability::NotApplicable,
                Some(format!(
                    "Requirement scoped to architecture '{}', machine architecture is '{}'",
                    arch, machine_arch
                )),
            ),
            ArchitectureMatch::Unknown => (
                EnvironmentApplicability::Unknown,
                Some(format!(
                    "Unknown architecture condition '{}' cannot be determined against machine architecture '{}'",
                    arch, machine_arch
                )),
            ),
        }
    } else {
        (EnvironmentApplicability::Applicable, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_architecture_normalization() {
        assert_eq!(
            Architecture::normalize_from_str("x86_64"),
            Architecture::X86_64
        );
        assert_eq!(
            Architecture::normalize_from_str("amd64"),
            Architecture::X86_64
        );
        assert_eq!(
            Architecture::normalize_from_str("x86-64"),
            Architecture::X86_64
        );
        assert_eq!(
            Architecture::normalize_from_str("x64"),
            Architecture::X86_64
        );

        assert_eq!(
            Architecture::normalize_from_str("aarch64"),
            Architecture::Aarch64
        );
        assert_eq!(
            Architecture::normalize_from_str("arm64"),
            Architecture::Aarch64
        );
        assert_eq!(
            Architecture::normalize_from_str("arm64e"),
            Architecture::Aarch64
        );
        assert_eq!(
            Architecture::normalize_from_str("armv8-a"),
            Architecture::Aarch64
        );

        assert_eq!(
            Architecture::normalize_from_str("armv7"),
            Architecture::Armv7
        );
        assert_eq!(
            Architecture::normalize_from_str("armv7l"),
            Architecture::Armv7
        );
        assert_eq!(
            Architecture::normalize_from_str("armhf"),
            Architecture::Armv7
        );

        assert_eq!(
            Architecture::normalize_from_str("riscv64"),
            Architecture::Riscv64
        );
        assert_eq!(
            Architecture::normalize_from_str("ppc64le"),
            Architecture::Ppc64le
        );
    }

    #[test]
    fn test_arm_architectures_distinct() {
        assert_ne!(
            Architecture::normalize_from_str("armv7"),
            Architecture::normalize_from_str("aarch64")
        );
        assert_ne!(
            Architecture::normalize_from_str("armv6"),
            Architecture::normalize_from_str("armv7")
        );
    }

    #[test]
    fn test_match_architecture() {
        assert_eq!(
            match_architecture("x86_64", "amd64"),
            ArchitectureMatch::Matches
        );
        assert_eq!(
            match_architecture("arm64", "aarch64"),
            ArchitectureMatch::Matches
        );
        assert_eq!(
            match_architecture("x86_64", "aarch64"),
            ArchitectureMatch::Mismatch
        );
        assert_eq!(
            match_architecture("custom_isa_x", "x86_64"),
            ArchitectureMatch::Unknown
        );
        assert_eq!(
            match_architecture("custom_isa_x", "custom_isa_x"),
            ArchitectureMatch::Matches
        );
    }

    #[test]
    fn test_evaluate_applicability() {
        // Platform + Arch match
        let (app, _) =
            evaluate_applicability(Some("linux"), Some("x86_64"), "Linux", "linux", "amd64");
        assert_eq!(app, EnvironmentApplicability::Applicable);

        // Platform mismatch
        let (app, _) =
            evaluate_applicability(Some("windows"), Some("x86_64"), "Linux", "linux", "x86_64");
        assert_eq!(app, EnvironmentApplicability::NotApplicable);

        // Arch mismatch
        let (app, _) =
            evaluate_applicability(Some("linux"), Some("aarch64"), "Linux", "linux", "x86_64");
        assert_eq!(app, EnvironmentApplicability::NotApplicable);

        // Unknown arch
        let (app, _) =
            evaluate_applicability(None, Some("unknown_arch_z"), "Linux", "linux", "x86_64");
        assert_eq!(app, EnvironmentApplicability::Unknown);
    }
}

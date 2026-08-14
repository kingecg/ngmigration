//! Angular major-version catalog: package versions, toolchain requirements.
//!
//! Version numbers were verified against the npm registry (Aug 2026).
//! Entries `major >= 21` are marked `confirmed: false` where the exact
//! TypeScript / Node requirement is estimated; the tool warns about this.

/// Toolchain + package versions for one Angular major release.
#[derive(Debug, Clone)]
pub struct AngularMajor {
    pub major: u32,
    /// `@angular/core` and every monorepo package sharing its version.
    pub core: &'static str,
    /// `@angular/cli` and `@angular-devkit/*` (they share a version number).
    pub cli: &'static str,
    /// `@angular/material` / `@angular/cdk` (own line since 8.x; equal major).
    pub material: &'static str,
    /// `zone.js`.
    pub zone_js: &'static str,
    /// `rxjs`.
    pub rxjs: &'static str,
    /// `tslib`.
    pub tslib: &'static str,
    /// Minimum supported TypeScript (major, minor).
    pub typescript: (u32, u32),
    /// Maximum supported TypeScript (major, minor).
    pub typescript_max: (u32, u32),
    /// Required Node.js version (display string).
    pub node: &'static str,
    /// Whether catalog data is fully confirmed (true for <= 20).
    pub confirmed: bool,
}

pub const LATEST_MAJOR: u32 = 22;
pub const MIN_SUPPORTED_MAJOR: u32 = 6;

/// All supported Angular majors, oldest first.
pub const CATALOG: &[AngularMajor] = &[
    AngularMajor {
        major: 6,
        core: "6.1.10",
        cli: "6.2.9",
        material: "6.4.7",
        zone_js: "0.8.28",
        rxjs: "6.6.7",
        tslib: "1.10.0",
        typescript: (2, 7),
        typescript_max: (2, 9),
        node: ">= 8.9",
        confirmed: true,
    },
    AngularMajor {
        major: 7,
        core: "7.2.16",
        cli: "7.3.10",
        material: "7.3.7",
        zone_js: "0.8.29",
        rxjs: "6.6.7",
        tslib: "1.10.0",
        typescript: (3, 1),
        typescript_max: (3, 2),
        node: ">= 8.9",
        confirmed: true,
    },
    AngularMajor {
        major: 8,
        core: "8.2.14",
        cli: "8.3.29",
        material: "8.2.3",
        zone_js: "0.9.1",
        rxjs: "6.6.7",
        tslib: "1.10.0",
        typescript: (3, 4),
        typescript_max: (3, 5),
        node: ">= 10.9",
        confirmed: true,
    },
    AngularMajor {
        major: 9,
        core: "9.1.13",
        cli: "9.1.15",
        material: "9.2.4",
        zone_js: "0.10.3",
        rxjs: "6.6.7",
        tslib: "2.6.3",
        typescript: (3, 6),
        typescript_max: (3, 8),
        node: ">= 10.13",
        confirmed: true,
    },
    AngularMajor {
        major: 10,
        core: "10.2.5",
        cli: "10.2.4",
        material: "10.2.7",
        zone_js: "0.10.3",
        rxjs: "6.6.7",
        tslib: "2.6.3",
        typescript: (3, 9),
        typescript_max: (3, 9),
        node: ">= 10.13",
        confirmed: true,
    },
    AngularMajor {
        major: 11,
        core: "11.2.14",
        cli: "11.2.19",
        material: "11.2.13",
        zone_js: "0.11.8",
        rxjs: "6.6.7",
        tslib: "2.6.3",
        typescript: (4, 0),
        typescript_max: (4, 1),
        node: ">= 10.13",
        confirmed: true,
    },
    AngularMajor {
        major: 12,
        core: "12.2.17",
        cli: "12.2.18",
        material: "12.2.13",
        zone_js: "0.11.8",
        rxjs: "6.6.7",
        tslib: "2.6.3",
        typescript: (4, 2),
        typescript_max: (4, 3),
        node: ">= 12.14",
        confirmed: true,
    },
    AngularMajor {
        major: 13,
        core: "13.4.0",
        cli: "13.3.11",
        material: "13.3.9",
        zone_js: "0.11.8",
        rxjs: "7.5.7",
        tslib: "2.6.3",
        typescript: (4, 4),
        typescript_max: (4, 6),
        node: ">= 12.20",
        confirmed: true,
    },
    AngularMajor {
        major: 14,
        core: "14.3.0",
        cli: "14.2.13",
        material: "14.2.7",
        zone_js: "0.11.8",
        rxjs: "7.5.7",
        tslib: "2.6.3",
        typescript: (4, 6),
        typescript_max: (4, 8),
        node: ">= 14.15",
        confirmed: true,
    },
    AngularMajor {
        major: 15,
        core: "15.2.10",
        cli: "15.2.11",
        material: "15.2.9",
        zone_js: "0.13.3",
        rxjs: "7.5.7",
        tslib: "2.6.3",
        typescript: (4, 8),
        typescript_max: (5, 0),
        node: ">= 14.20",
        confirmed: true,
    },
    AngularMajor {
        major: 16,
        core: "16.2.12",
        cli: "16.2.16",
        material: "16.2.14",
        zone_js: "0.14.10",
        rxjs: "7.8.1",
        tslib: "2.6.3",
        typescript: (4, 9),
        typescript_max: (5, 1),
        node: ">= 16.14",
        confirmed: true,
    },
    AngularMajor {
        major: 17,
        core: "17.3.12",
        cli: "17.3.17",
        material: "17.3.10",
        zone_js: "0.14.10",
        rxjs: "7.8.1",
        tslib: "2.6.3",
        typescript: (5, 2),
        typescript_max: (5, 4),
        node: ">= 18.13",
        confirmed: true,
    },
    AngularMajor {
        major: 18,
        core: "18.2.14",
        cli: "18.2.21",
        material: "18.2.14",
        zone_js: "0.14.10",
        rxjs: "7.8.1",
        tslib: "2.6.3",
        typescript: (5, 4),
        typescript_max: (5, 5),
        node: ">= 18.19",
        confirmed: true,
    },
    AngularMajor {
        major: 19,
        core: "19.2.25",
        cli: "19.2.27",
        material: "19.2.19",
        zone_js: "0.15.1",
        rxjs: "7.8.1",
        tslib: "2.6.3",
        typescript: (5, 5),
        typescript_max: (5, 8),
        node: ">= 20.11",
        confirmed: true,
    },
    AngularMajor {
        major: 20,
        core: "20.3.27",
        cli: "20.3.34",
        material: "20.2.14",
        zone_js: "0.16.2",
        rxjs: "7.8.1",
        tslib: "2.6.3",
        typescript: (5, 8),
        typescript_max: (5, 9),
        node: ">= 20.19",
        confirmed: true,
    },
    AngularMajor {
        major: 21,
        core: "21.2.19",
        cli: "21.2.21",
        material: "21.2.14",
        zone_js: "0.16.2",
        rxjs: "7.8.1",
        tslib: "2.6.3",
        typescript: (5, 8),
        typescript_max: (5, 9),
        node: ">= 22.12",
        confirmed: false,
    },
    AngularMajor {
        major: 22,
        core: "22.1.1",
        cli: "22.1.3",
        material: "22.1.2",
        zone_js: "0.16.2",
        rxjs: "7.8.1",
        tslib: "2.6.3",
        typescript: (5, 9),
        typescript_max: (5, 9),
        node: ">= 22.12",
        confirmed: false,
    },
];

/// Look up a catalog entry by major.
pub fn catalog_major(major: u32) -> Option<&'static AngularMajor> {
    CATALOG.iter().find(|e| e.major == major)
}

/// Highest supported major.
pub fn latest_major() -> u32 {
    CATALOG.iter().map(|e| e.major).max().unwrap_or(0)
}

/// The `@angular/*` monorepo packages that share `@angular/core`'s version.
pub const MONOREPO_PACKAGES: &[&str] = &[
    "@angular/core",
    "@angular/common",
    "@angular/compiler",
    "@angular/compiler-cli",
    "@angular/animations",
    "@angular/forms",
    "@angular/router",
    "@angular/platform-browser",
    "@angular/platform-browser-dynamic",
    "@angular/platform-server",
    "@angular/elements",
    "@angular/service-worker",
    "@angular/upgrade",
    "@angular/localize",
    "@angular/ssr",
];

/// The `@angular/material` / `@angular/cdk` pair, which track Angular's major.
pub const MATERIAL_PACKAGES: &[&str] = &["@angular/material", "@angular/cdk"];

/// TypeScript devDependency specifier for a major, e.g. `~5.8.0` (pins to the
/// newest supported TypeScript, mirroring what `ng update` produces).
pub fn typescript_spec(cat: &AngularMajor) -> String {
    format!("~{}.{}.0", cat.typescript_max.0, cat.typescript_max.1)
}

/// Display string of the supported TypeScript range, e.g. `5.5 - 5.8`.
pub fn typescript_range(cat: &AngularMajor) -> String {
    let min = format!("{}.{}", cat.typescript.0, cat.typescript.1);
    let max = format!("{}.{}", cat.typescript_max.0, cat.typescript_max.1);
    if min == max {
        min
    } else {
        format!("{min} - {max}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_contiguous_from_6() {
        let mut last = MIN_SUPPORTED_MAJOR;
        for entry in CATALOG {
            assert_eq!(entry.major, last, "catalog must be contiguous");
            last += 1;
        }
        assert_eq!(last - 1, LATEST_MAJOR);
    }

    #[test]
    fn versions_are_three_part() {
        for e in CATALOG {
            let parts: Vec<&str> = e.core.split('.').collect();
            assert_eq!(parts.len(), 3, "core {} should be x.y.z", e.core);
            assert!(e.material.split('.').count() >= 3);
        }
    }

    #[test]
    fn typescript_spec_format() {
        let e = catalog_major(19).unwrap();
        assert_eq!(typescript_spec(e), "~5.8.0");
        assert_eq!(typescript_range(e), "5.5 - 5.8");
    }
}

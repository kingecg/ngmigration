//! Third-party dependency compatibility.
//!
//! Angular-ecosystem libraries (`@ngrx/*`, `ng-zorro-antd`, ...) tie their
//! release cadence to Angular's major version. This module computes
//! recommended version bumps based on:
//!   1. a curated local database (offline, always available);
//!   2. an optional npm-registry peerDependency check (offline-safe by default).
//!
//! Suggestions are advisory: they are listed in the plan/report and applied
//! only with `--apply-recommended`.

use crate::model::{DependencySuggestion, Project};

/// Package major == Angular major for majors >= `from`.
const TRACKS_MAJOR: &[(&str, u32)] = &[
    ("@ngrx/store", 8),
    ("@ngrx/effects", 8),
    ("@ngrx/router-store", 8),
    ("@ngrx/entity", 8),
    ("@ngrx/component-store", 8),
    ("@ngrx/store-devtools", 8),
    ("ng-zorro-antd", 8),
    ("primeng", 12),
    ("ngx-toastr", 15),
];

/// Packages the align step owns; never suggested here.
fn is_managed(package: &str) -> bool {
    package.starts_with("@angular/")
        || package.starts_with("@angular-devkit/")
        || matches!(
            package,
            "rxjs" | "zone.js" | "typescript" | "tslib" | "@types/node" | "@types/jasmine"
        )
}

/// Compute advisory suggestions for third-party dependencies, offline.
pub fn suggest(project: &Project, target_major: u32) -> Vec<DependencySuggestion> {
    let mut out = Vec::new();
    for (package, spec) in project.package.all_dependencies() {
        if is_managed(&package) {
            continue;
        }
        let Some(current) = crate::model::parse_major(&spec) else {
            continue;
        };
        if let Some((_, from)) = TRACKS_MAJOR.iter().find(|(p, _)| *p == package.as_str()) {
            if current >= target_major || target_major < *from {
                continue;
            }
            out.push(DependencySuggestion {
                package,
                from: spec,
                to: format!("^{target_major}.0.0"),
                reason: format!(
                    "this library tracks the Angular major version (from v{from}); \
                     bump to match Angular {target_major}"
                ),
            });
        }
    }
    out.sort_by(|a, b| a.package.cmp(&b.package));
    out
}

/// Augment local suggestions with npm-registry peerDependency lookups for any
/// dependency that still has an unresolved Angular coupling. Requires the
/// `network` feature; returns local-only results otherwise.
pub fn suggest_with_registry(
    project: &Project,
    target_major: u32,
    offline: bool,
) -> Vec<DependencySuggestion> {
    let mut out = suggest(project, target_major);
    if offline || !cfg!(feature = "network") {
        return out;
    }
    let already: Vec<String> = out.iter().map(|s| s.package.clone()).collect();
    for (package, spec) in project.package.all_dependencies() {
        if is_managed(&package) || already.contains(&package) {
            continue;
        }
        let Some(current) = crate::model::parse_major(&spec) else {
            continue;
        };
        if let Some(candidate) = crate::npm::find_compatible_version(&package, target_major) {
            let cand_major = crate::model::parse_major(&candidate).unwrap_or(0);
            if cand_major > current {
                out.push(DependencySuggestion {
                    package,
                    from: spec,
                    to: candidate,
                    reason: format!(
                        "latest version with a peerDependency on @angular/core compatible \
                         with Angular {target_major}"
                    ),
                });
            }
        }
    }
    out.sort_by(|a, b| a.package.cmp(&b.package));
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::detect;

    fn project_with(deps: &str) -> Project {
        let dir = tempfile::TempDir::new().unwrap();
        let pkg = format!(
            r#"{{
  "name": "t",
  "dependencies": {deps},
  "devDependencies": {{ "@angular/cli": "^12.2.0" }}
}}"#
        );
        std::fs::write(dir.path().join("package.json"), pkg).unwrap();
        detect::detect(Path::new(dir.path())).unwrap()
    }

    #[test]
    fn suggests_ngrx_bump() {
        let p = project_with(r#"{ "@angular/core": "12.2.0", "@ngrx/store": "^12.5.0" }"#);
        let s = suggest(&p, 20);
        assert!(
            s.iter()
                .any(|x| x.package == "@ngrx/store" && x.to == "^20.0.0"),
            "got {s:?}"
        );
    }

    #[test]
    fn no_suggestion_when_already_latest() {
        let p = project_with(r#"{ "@angular/core": "20.0.0", "@ngrx/store": "^20.1.0" }"#);
        assert!(suggest(&p, 20).is_empty());
    }

    #[test]
    fn ignores_managed_packages() {
        let p = project_with(r#"{ "@angular/core": "12.2.0", "rxjs": "^6.6.0" }"#);
        assert!(suggest(&p, 20).is_empty());
    }

    #[test]
    fn ignores_unknown_libs_offline() {
        let p = project_with(r#"{ "@angular/core": "12.2.0", "some-lib": "^3.0.0" }"#);
        assert!(suggest(&p, 20).is_empty());
    }
}

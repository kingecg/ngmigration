//! Integration tests against the committed `fixtures/ng12-app` project.

use std::fs;
use std::path::Path;

use angular_migrator::detect;
use angular_migrator::migrate::{self, MigrateOptions};
use angular_migrator::plan;

fn fixture() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/ng12-app")
        .leak()
}

/// Copy the fixture into a fresh temp dir so the committed files stay pristine.
fn fixture_copy() -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    copy_dir(fixture(), dir.path());
    dir
}

fn copy_dir(src: &Path, dst: &Path) {
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.unwrap();
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).unwrap();
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

#[test]
fn detects_fixture_as_angular_12() {
    let project = detect::detect(fixture()).unwrap();
    assert_eq!(project.angular_major(), Some(12));
    assert_eq!(project.cli_major(), Some(12));
    assert!(project.has_e2e_target());
    assert_eq!(project.tsconfig_paths.len(), 3);
}

#[test]
fn plan_12_to_22_visits_every_major() {
    let project = detect::detect(fixture()).unwrap();
    let plan = plan::plan(&project, Some(22)).unwrap();
    assert_eq!(plan.from, 12);
    assert_eq!(plan.to, 22);
    assert_eq!(plan.path(), vec![13, 14, 15, 16, 17, 18, 19, 20, 21, 22]);
    // Every step must carry at least a note.
    for step in &plan.steps {
        assert!(!step.rules.is_empty());
    }
    // Third-party suggestions include @ngrx/store.
    assert!(plan.third_party.iter().any(|s| s.package == "@ngrx/store"));
}

#[test]
fn plan_rejects_target_below_current() {
    let project = detect::detect(fixture()).unwrap();
    assert!(plan::plan(&project, Some(11)).is_err());
}

#[test]
fn migrate_fixture_to_17_end_to_end() {
    let dir = fixture_copy();
    let mut project = detect::detect(dir.path()).unwrap();
    let opts = MigrateOptions {
        dry_run: false,
        offline: true,
        apply_control_flow: true,
        apply_recommended: false,
    };
    let outcome = migrate::migrate(&mut project, Some(17), &opts).unwrap();

    assert!(outcome.package_rewritten);
    assert!(outcome.workspace_rewritten);
    assert!(
        outcome.control_flow_migrated >= 2,
        "got {}",
        outcome.control_flow_migrated
    );
    assert!(
        outcome.control_flow_skipped >= 1,
        "got {}",
        outcome.control_flow_skipped
    );

    let root = dir.path();
    let pkg: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("package.json")).unwrap()).unwrap();

    // Core deps aligned to v17.
    assert_eq!(pkg["dependencies"]["@angular/core"], "^17.3.12");
    assert_eq!(pkg["dependencies"]["@angular/router"], "^17.3.12");
    assert_eq!(pkg["dependencies"]["@angular/material"], "^17.3.10");
    assert_eq!(pkg["dependencies"]["rxjs"], "^7.8.1");
    assert_eq!(pkg["dependencies"]["zone.js"], "^0.14.10");
    assert_eq!(pkg["devDependencies"]["typescript"], "~5.4.0");

    // Removed: protractor, @angular/language-service, e2e script.
    assert!(pkg["dependencies"]
        .get("@angular/language-service")
        .is_none());
    assert!(pkg["devDependencies"].get("protractor").is_none());
    assert!(pkg["scripts"].get("e2e").is_none());
    // build script had --prod stripped.
    assert_eq!(pkg["scripts"]["build"], "ng build");

    // angular.json: e2e target gone.
    let ws: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("angular.json")).unwrap()).unwrap();
    assert!(ws["projects"]["ng12-app"]["architect"].get("e2e").is_none());

    // tsconfig: enableIvy removed, fullTemplateTypeCheck untouched.
    let ts = fs::read_to_string(root.join("tsconfig.json")).unwrap();
    assert!(!ts.contains("enableIvy"));
    assert!(ts.contains("fullTemplateTypeCheck"));

    // @NgModule entryComponents removed.
    let module = fs::read_to_string(root.join("src/app/app.module.ts")).unwrap();
    assert!(!module.contains("entryComponents"));

    // Control flow: *ngIf/*ngFor migrated; the `else` variant left intact.
    let html = fs::read_to_string(root.join("src/app/app.component.html")).unwrap();
    assert!(html.contains("@if (loaded) {"));
    assert!(html.contains("@for (item of items; track item) {"));
    assert!(html.contains("*ngIf=\"title === 'ng12-app'; else fallback\""));

    // Third-party suggestions surfaced.
    assert!(outcome
        .third_party
        .iter()
        .any(|s| s.package == "@ngrx/store"));
}

#[test]
fn migrate_with_apply_recommended_bumps_ngrx() {
    let dir = fixture_copy();
    let mut project = detect::detect(dir.path()).unwrap();
    let opts = MigrateOptions {
        dry_run: false,
        offline: true,
        apply_recommended: true,
        apply_control_flow: false,
    };
    migrate::migrate(&mut project, Some(20), &opts).unwrap();
    let pkg: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(pkg["dependencies"]["@ngrx/store"], "^20.0.0");
}

#[test]
fn dry_run_is_immutable() {
    let dir = fixture_copy();
    let snapshot = snapshot_dir(dir.path());
    let mut project = detect::detect(dir.path()).unwrap();
    let opts = MigrateOptions {
        dry_run: true,
        ..Default::default()
    };
    migrate::migrate(&mut project, Some(20), &opts).unwrap();
    assert_eq!(snapshot, snapshot_dir(dir.path()));
}

fn snapshot_dir(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let rel = entry
                .path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let content = fs::read_to_string(entry.path()).unwrap();
            out.push((rel, content));
        }
    }
    out.sort();
    out
}

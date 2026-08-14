//! Migration orchestration: apply a [`MigrationPlan`] to a project on disk.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::catalog;
use crate::control_flow;
use crate::dependencies;
use crate::detect;
use crate::model::{DependencySuggestion, MigrationRule, Project};
use crate::plan;
use crate::thirdparty;
use crate::transforms;

/// Options for the `migrate` command.
#[derive(Debug, Clone, Default)]
pub struct MigrateOptions {
    pub dry_run: bool,
    /// Apply third-party dependency suggestions.
    pub apply_recommended: bool,
    /// Skip npm-registry lookups.
    pub offline: bool,
    /// Also run the *ngIf/*ngFor -> @if/@for rewrite.
    pub apply_control_flow: bool,
}

/// The result of running a migration.
#[derive(Debug, Default)]
pub struct MigrationOutcome {
    pub log: Vec<String>,
    pub changed_files: Vec<PathBuf>,
    pub notes: Vec<String>,
    pub third_party: Vec<DependencySuggestion>,
    pub warnings: Vec<String>,
    pub control_flow_migrated: usize,
    pub control_flow_skipped: usize,
    pub package_rewritten: bool,
    pub workspace_rewritten: bool,
    pub dry_run: bool,
    /// Whether any in-memory package.json mutation happened.
    pub package_dirty: bool,
}

/// Detect the project at `root` and run the full migration.
pub fn run(root: &Path, target: Option<u32>, opts: &MigrateOptions) -> Result<MigrationOutcome> {
    let mut project = detect::detect(root)?;
    migrate(&mut project, target, opts)
}

/// Run the migration against an already-detected project.
pub fn migrate(
    project: &mut Project,
    target: Option<u32>,
    opts: &MigrateOptions,
) -> Result<MigrationOutcome> {
    let plan = if opts.apply_control_flow {
        plan::plan_with_control_flow(project, target)?
    } else {
        plan::plan(project, target)?
    };

    let mut outcome = MigrationOutcome {
        dry_run: opts.dry_run,
        ..Default::default()
    };

    outcome.log.push(format!(
        "Migrating Angular {} -> {} ({}{})",
        plan.from,
        plan.to,
        plan.path()
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<_>>()
            .join(" -> "),
        if opts.dry_run { " [dry run]" } else { "" }
    ));

    // Aggregate tsconfig angularCompilerOptions edits.
    let mut compiler_removes: Vec<String> = Vec::new();
    let mut compiler_sets: BTreeMap<String, String> = BTreeMap::new();

    for step in &plan.steps {
        outcome
            .log
            .push(format!("--- Angular {} -> {} ---", step.from, step.to));
        for rule in &step.rules {
            apply_rule(
                project,
                rule,
                opts,
                &mut compiler_removes,
                &mut compiler_sets,
                &mut outcome,
            )?;
        }
    }

    // Compiler-option edits (tsconfig files).
    if !compiler_removes.is_empty() || !compiler_sets.is_empty() {
        let changed = crate::tsconfig::apply_compiler_options(
            project,
            &compiler_removes,
            &compiler_sets,
            opts.dry_run,
        )?;
        for f in changed {
            outcome.log.push(format!(
                "  ~ updated angularCompilerOptions in {}",
                rel(root(project), &f)
            ));
            outcome.changed_files.push(f);
        }
    }

    // Version alignment to the target major.
    let aligned = dependencies::align_dependencies(&mut project.package, plan.to);
    if aligned > 0 {
        outcome.package_dirty = true;
        outcome.log.push(format!(
            "~ aligned {aligned} Angular/toolchain dependencies to v{}",
            plan.to
        ));
    }

    // Third-party dependency suggestions.
    let suggestions = thirdparty::suggest_with_registry(project, plan.to, opts.offline);
    outcome.third_party = suggestions.clone();
    for s in &suggestions {
        outcome.log.push(format!(
            "? third-party: {} {} -> {} ({})",
            s.package, s.from, s.to, s.reason
        ));
    }
    if opts.apply_recommended && !suggestions.is_empty() {
        let applied = dependencies::apply_suggestions(&mut project.package, &suggestions);
        if applied > 0 {
            outcome.package_dirty = true;
            outcome
                .log
                .push(format!("~ applied {applied} third-party recommendations"));
        }
    }

    // Persist package.json (unless a dry run).
    if outcome.package_dirty {
        dependencies::save(&project.package, opts.dry_run)?;
        outcome.package_rewritten = true;
        outcome.log.push(format!(
            "{} package.json",
            if opts.dry_run {
                "would rewrite"
            } else {
                "rewrote"
            }
        ));
    }

    // Persist angular.json workspace edits.
    if let (Some(wp), Some(ws)) = (&project.workspace_path, &project.workspace) {
        let pretty = serde_json::to_string_pretty(ws)? + "\n";
        if pretty != std::fs::read_to_string(wp).unwrap_or_default() {
            if !opts.dry_run {
                std::fs::write(wp, pretty)?;
            }
            outcome.workspace_rewritten = true;
            outcome.changed_files.push(wp.clone());
            outcome.log.push(format!(
                "{} {}",
                if opts.dry_run {
                    "would rewrite"
                } else {
                    "rewrote"
                },
                wp.display()
            ));
        }
    }

    // Collect manual-step notes.
    outcome.notes = plan.notes().into_iter().map(|s| s.to_string()).collect();

    // Catalog confidence warning.
    if let Some(cat) = catalog::catalog_major(plan.to) {
        if !cat.confirmed {
            outcome.warnings.push(format!(
                "Angular v{} is newer than this tool's verified data; double-check the \
                 TypeScript/Node versions and breaking changes against https://update.angular.io",
                plan.to
            ));
        }
    }

    Ok(outcome)
}

fn root(project: &Project) -> &Path {
    &project.root
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

#[allow(clippy::too_many_arguments)]
fn apply_rule(
    project: &mut Project,
    rule: &MigrationRule,
    opts: &MigrateOptions,
    compiler_removes: &mut Vec<String>,
    compiler_sets: &mut BTreeMap<String, String>,
    outcome: &mut MigrationOutcome,
) -> Result<()> {
    use MigrationRule as R;
    match rule {
        R::DepUpdate { package, version } => {
            if dependencies::apply_dep_rules(&mut project.package, std::slice::from_ref(rule)) > 0 {
                outcome.package_dirty = true;
                outcome
                    .log
                    .push(format!("  - dependency {package} -> {version}"));
            }
        }
        R::DepRemove { package } => {
            if dependencies::apply_dep_rules(&mut project.package, std::slice::from_ref(rule)) > 0 {
                outcome.package_dirty = true;
                outcome
                    .log
                    .push(format!("  - removed dependency {package}"));
            }
        }
        R::DepAdd {
            package, version, ..
        } => {
            if dependencies::apply_dep_rules(&mut project.package, std::slice::from_ref(rule)) > 0 {
                outcome.package_dirty = true;
                outcome
                    .log
                    .push(format!("  - added dependency {package}@{version}"));
            }
        }
        R::Replace {
            glob,
            pattern,
            replacement,
            kind,
        } => {
            let changed = transforms::apply_regex_replace(
                &project.root,
                glob,
                pattern,
                replacement,
                *kind,
                opts.dry_run,
            )?;
            for f in &changed {
                outcome
                    .log
                    .push(format!("  ~ rewrote {}", rel(&project.root, f)));
                outcome.changed_files.push(f.clone());
            }
        }
        R::RemoveNgModuleField { field } => {
            let changed =
                transforms::apply_remove_ng_module_field(&project.root, field, opts.dry_run)?;
            for f in &changed {
                outcome.log.push(format!(
                    "  ~ removed `{field}` in {}",
                    rel(&project.root, f)
                ));
                outcome.changed_files.push(f.clone());
            }
        }
        R::RemoveCompilerOption { key } => {
            if !compiler_removes.contains(key) {
                compiler_removes.push(key.clone());
            }
        }
        R::SetCompilerOption { key, value } => {
            compiler_sets.insert(key.clone(), value.clone());
        }
        R::StripScriptFlags { .. } => {
            let before = project.package.scripts();
            dependencies::apply_script_rules(&mut project.package, std::slice::from_ref(rule));
            let after = project.package.scripts();
            for (name, val) in &before {
                if after.get(name) != Some(val) {
                    outcome.package_dirty = true;
                    outcome.log.push(format!(
                        "  - script `{name}`: stripped deprecated flags -> `{}`",
                        after.get(name).unwrap_or(&String::new())
                    ));
                }
            }
        }
        R::RemoveScript { script } => {
            if project.package.scripts().contains_key(script) {
                dependencies::apply_script_rules(&mut project.package, std::slice::from_ref(rule));
                outcome.package_dirty = true;
                outcome.log.push(format!("  - removed script `{script}`"));
            }
        }
        R::RemoveWorkspaceTarget { target } => {
            if let Some(ws) = &mut project.workspace {
                if remove_workspace_target(ws, target) {
                    outcome.log.push(format!(
                        "  - removed `{target}` target from angular.json projects"
                    ));
                }
            }
        }
        R::ControlFlowMigration => {
            let result = control_flow::apply_control_flow(&project.root, opts.dry_run)?;
            outcome.control_flow_migrated += result.migrated;
            outcome.control_flow_skipped += result.skipped;
            outcome.warnings.extend(result.warnings);
            for f in result.changed {
                outcome
                    .log
                    .push(format!("  ~ control flow: {}", rel(&project.root, &f)));
                outcome.changed_files.push(f);
            }
        }
        R::Note { text } => {
            outcome.log.push(format!("  ! note: {text}"));
        }
    }
    Ok(())
}

/// Remove an architect target (e.g. `e2e`) from every project. Returns true
/// when anything changed.
fn remove_workspace_target(ws: &mut serde_json::Value, target: &str) -> bool {
    let mut changed = false;
    if let Some(projects) = ws.get_mut("projects").and_then(|p| p.as_object_mut()) {
        for proj in projects.values_mut() {
            if let Some(architect) = proj.get_mut("architect").and_then(|a| a.as_object_mut()) {
                if architect.remove(target).is_some() {
                    changed = true;
                }
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn fixture_root() -> TempDir {
        let dir = TempDir::new().unwrap();
        let r = dir.path();
        let _ = fs::write(
            r.join("package.json"),
            r#"{
  "name": "ng12-app",
  "version": "0.1.0",
  "scripts": {
    "ng": "ng",
    "start": "ng serve",
    "build": "ng build --prod",
    "test": "ng test",
    "e2e": "ng e2e"
  },
  "private": true,
  "dependencies": {
    "@angular/animations": "^12.2.0",
    "@angular/common": "^12.2.0",
    "@angular/compiler": "^12.2.0",
    "@angular/core": "^12.2.0",
    "@angular/forms": "^12.2.0",
    "@angular/material": "^12.2.0",
    "@angular/platform-browser": "^12.2.0",
    "@angular/platform-browser-dynamic": "^12.2.0",
    "@angular/router": "^12.2.0",
    "@ngrx/store": "^12.5.0",
    "rxjs": "~6.6.0",
    "tslib": "^2.3.0",
    "zone.js": "~0.11.4"
  },
  "devDependencies": {
    "@angular-devkit/build-angular": "^12.2.0",
    "@angular/cli": "^12.2.0",
    "@angular/compiler-cli": "^12.2.0",
    "typescript": "~4.3.0",
    "protractor": "~7.0.0"
  }
}"#,
        );
        let _ = fs::write(
            r.join("angular.json"),
            r#"{
  "version": 1,
  "projects": {
    "ng12-app": {
      "architect": {
        "build": { "builder": "@angular-devkit/build-angular:browser" },
        "e2e": { "builder": "@angular-devkit/build-angular:protractor" }
      }
    }
  }
}"#,
        );
        let _ = fs::write(
            r.join("tsconfig.json"),
            r#"{
  "compileOnSave": false,
  "compilerOptions": { "baseUrl": "./", "outDir": "./dist/out-tsc" },
  "angularCompilerOptions": {
    "enableIvy": true,
    "fullTemplateTypeCheck": true
  }
}"#,
        );
        fs::create_dir_all(r.join("src/app")).unwrap();
        let _ = fs::write(
            r.join("src/app/app.module.ts"),
            r#"import { NgModule } from '@angular/core';
import { BrowserModule } from '@angular/platform-browser';
import { AppComponent } from './app.component';

@NgModule({
  declarations: [AppComponent],
  imports: [BrowserModule],
  entryComponents: [AppComponent],
  providers: [],
  bootstrap: [AppComponent]
})
export class AppModule { }
"#,
        );
        let _ = fs::write(
            r.join("src/app/app.component.html"),
            r#"<div class="wrapper" *ngIf="loaded">
  <h1>{{ title }}</h1>
  <ul>
    <li *ngFor="let item of items">{{ item }}</li>
  </ul>
</div>
"#,
        );
        let _ = fs::write(
            r.join("src/app/app.component.ts"),
            "export class AppComponent { loaded = true; }\n",
        );
        dir
    }

    #[test]
    fn full_migration_12_to_17() {
        let dir = fixture_root();
        let mut project = detect::detect(dir.path()).unwrap();
        let opts = MigrateOptions {
            dry_run: false,
            apply_recommended: false,
            offline: true,
            apply_control_flow: true,
        };
        let outcome = migrate(&mut project, Some(17), &opts).unwrap();

        // package.json assertions
        let pkg_raw = fs::read_to_string(dir.path().join("package.json")).unwrap();
        assert!(pkg_raw.contains("\"@angular/core\": \"^17.3.12\""));
        assert!(pkg_raw.contains("\"rxjs\": \"^7.8.1\""));
        assert!(pkg_raw.contains("\"zone.js\": \"^0.14.10\""));
        assert!(pkg_raw.contains("\"typescript\": \"~5.4.0\""));
        assert!(!pkg_raw.contains("protractor"));
        assert!(!pkg_raw.contains("\"e2e\""));

        // angular.json: e2e target removed
        let ws_raw = fs::read_to_string(dir.path().join("angular.json")).unwrap();
        assert!(!ws_raw.contains("\"e2e\""));

        // tsconfig: enableIvy removed
        let ts_raw = fs::read_to_string(dir.path().join("tsconfig.json")).unwrap();
        assert!(!ts_raw.contains("enableIvy"));
        assert!(ts_raw.contains("fullTemplateTypeCheck"));

        // app.module.ts: entryComponents removed
        let module_raw = fs::read_to_string(dir.path().join("src/app/app.module.ts")).unwrap();
        assert!(!module_raw.contains("entryComponents"));

        // control flow applied
        let html_raw = fs::read_to_string(dir.path().join("src/app/app.component.html")).unwrap();
        assert!(html_raw.contains("@if (loaded) {"));
        assert!(html_raw.contains("@for (item of items; track item) {"));
        assert!(!html_raw.contains("*ngIf"));

        // third-party suggestion present
        assert!(outcome
            .third_party
            .iter()
            .any(|s| s.package == "@ngrx/store"));
    }

    #[test]
    fn dry_run_writes_nothing() {
        let dir = fixture_root();
        let before_pkg = fs::read_to_string(dir.path().join("package.json")).unwrap();
        let mut project = detect::detect(dir.path()).unwrap();
        let opts = MigrateOptions {
            dry_run: true,
            ..Default::default()
        };
        migrate(&mut project, Some(17), &opts).unwrap();
        let after_pkg = fs::read_to_string(dir.path().join("package.json")).unwrap();
        assert_eq!(before_pkg, after_pkg);
    }
}

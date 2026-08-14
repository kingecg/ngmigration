//! Migration planning: turn a detected project + target major into an ordered
//! [`MigrationPlan`] that visits every intermediate major version.

use anyhow::Result;

use crate::catalog;
use crate::detect;
use crate::model::{MajorStep, MigrationPlan};
use crate::rules::{self, PlanOptions};
use crate::thirdparty;

/// Compute the migration plan from the project's current Angular major to
/// `target`. When `target` is `None`, the newest supported major is used.
pub fn plan(project: &crate::model::Project, target: Option<u32>) -> Result<MigrationPlan> {
    let from = project
        .angular_major()
        .ok_or_else(|| anyhow::anyhow!("no `@angular/core` dependency found in package.json"))?;

    detect::ensure_supported(from)?;

    let to = target.unwrap_or_else(catalog::latest_major);
    if to < from {
        anyhow::bail!("target major {to} is older than the detected Angular {from}");
    }
    if catalog::catalog_major(to).is_none() {
        anyhow::bail!(
            "target major {to} is not supported by this tool (newest: {})",
            catalog::latest_major()
        );
    }

    let opts = PlanOptions {
        apply_control_flow: false,
    };

    let mut steps = Vec::new();
    for major in from..to {
        let rules = rules::step_rules(major, opts);
        steps.push(MajorStep {
            from: major,
            to: major + 1,
            rules,
        });
    }

    let third_party = thirdparty::suggest(project, to);

    Ok(MigrationPlan {
        from,
        to,
        steps,
        third_party,
    })
}

/// Rebuild the plan with control-flow migration enabled (Angular 17 step).
pub fn plan_with_control_flow(
    project: &crate::model::Project,
    target: Option<u32>,
) -> Result<MigrationPlan> {
    let mut plan = plan(project, target)?;
    let opts = PlanOptions {
        apply_control_flow: true,
    };
    for step in &mut plan.steps {
        if step.from == 16 {
            step.rules = rules::step_rules(16, opts);
        }
    }
    Ok(plan)
}

/// Concise human-readable rendering of the plan.
pub fn render_plan(plan: &MigrationPlan, root: &std::path::Path) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Migration path: Angular {} -> {}\n",
        plan.from, plan.to
    ));
    let path = plan
        .path()
        .iter()
        .map(|m| m.to_string())
        .collect::<Vec<_>>()
        .join(" -> ");
    out.push_str(&format!("  {}\n\n", path));
    for step in &plan.steps {
        out.push_str(&format!("[{} -> {}]\n", step.from, step.to));
        let actions = step
            .rules
            .iter()
            .filter(|r| !matches!(r, crate::model::MigrationRule::Note { .. }));
        let notes = step.notes();
        if actions.clone().count() == 0 {
            out.push_str("  (no automated changes)\n");
        }
        for rule in actions {
            out.push_str(&format!("  - {}\n", describe_rule(rule)));
        }
        for note in notes {
            out.push_str(&format!("  ! {}\n", note));
        }
        out.push('\n');
    }
    if !plan.third_party.is_empty() {
        out.push_str("Third-party dependencies:\n");
        for s in &plan.third_party {
            out.push_str(&format!(
                "  ~ {}: {} -> {} ({})\n",
                s.package, s.from, s.to, s.reason
            ));
        }
    }
    out.push_str(&format!("\nProject root: {}\n", root.display()));
    out
}

/// Concise human-readable rendering of a single rule.
pub fn describe_rule(rule: &crate::model::MigrationRule) -> String {
    use crate::model::MigrationRule as R;
    match rule {
        R::DepUpdate { package, version } => format!("update {package} to {version}"),
        R::DepRemove { package } => format!("remove dependency {package}"),
        R::DepAdd {
            package, version, ..
        } => format!("add dependency {package}@{version}"),
        R::Replace { glob, pattern, .. } => format!("regex replace `{pattern}` in {glob}"),
        R::RemoveNgModuleField { field } => {
            format!("remove `{field}` from every @NgModule metadata")
        }
        R::RemoveCompilerOption { key } => {
            format!("remove `{key}` from angularCompilerOptions")
        }
        R::SetCompilerOption { key, value } => {
            format!("set angularCompilerOptions.{key} = {value}")
        }
        R::StripScriptFlags { flags } => {
            format!("strip deprecated CLI flags from scripts: {flags:?}")
        }
        R::RemoveScript { script } => format!("remove the `{script}` npm script"),
        R::RemoveWorkspaceTarget { target } => {
            format!("remove the `{target}` target from all projects in angular.json")
        }
        R::ControlFlowMigration => "*ngIf/*ngFor -> @if/@for control-flow rewrite".into(),
        R::Note { .. } => unreachable!("notes are rendered separately"),
    }
}

/// Deterministic summary used by tests.
pub fn plan_signature(plan: &MigrationPlan) -> String {
    let mut sig = format!("{}-{}", plan.from, plan.to);
    for step in &plan.steps {
        sig.push_str(&format!("/{}/{}", step.from, step.rules.len()));
    }
    sig
}

/// Convenience for tests: the set of dependency suggestions as strings.
pub fn suggestions_sig(plan: &MigrationPlan) -> Vec<String> {
    plan.third_party
        .iter()
        .map(|s| format!("{}:{}->{}", s.package, s.from, s.to))
        .collect()
}

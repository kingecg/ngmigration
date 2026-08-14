//! Command-line interface (clap-based).

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::catalog;
use crate::detect;
use crate::migrate::{self, MigrateOptions};
use crate::plan;
use crate::report;

#[derive(Parser)]
#[command(
    name = "angular-migrator",
    version,
    about = "Offline-first Angular project migration tool: major-version upgrades with third-party dependency resolution"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Inspect the project: detected Angular / CLI / TS / Node versions and layout.
    Analyze {
        /// Project root (defaults to current directory).
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Show the step-by-step migration plan without changing anything.
    Plan {
        /// Project root.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Target Angular major (defaults to the newest supported).
        #[arg(long)]
        target: Option<u32>,
        /// Write the plan as Markdown to this file.
        #[arg(long)]
        report: Option<PathBuf>,
    },
    /// Apply the migration to the project.
    Migrate {
        /// Project root.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Target Angular major (defaults to the newest supported).
        #[arg(long)]
        target: Option<u32>,
        /// Print what would change without writing anything.
        #[arg(long)]
        dry_run: bool,
        /// Skip npm-registry lookups for third-party dependencies.
        #[arg(long)]
        offline: bool,
        /// Apply recommended third-party dependency bumps.
        #[arg(long)]
        apply_recommended: bool,
        /// Also rewrite *ngIf/*ngFor to @if/@for control-flow blocks.
        #[arg(long)]
        apply_control_flow: bool,
        /// Write a Markdown report to this file.
        #[arg(long)]
        report: Option<PathBuf>,
    },
    /// List the supported Angular majors and their package versions.
    Catalog,
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Analyze { path } => analyze(&path),
        Commands::Plan {
            path,
            target,
            report,
        } => plan_cmd(&path, target, report),
        Commands::Migrate {
            path,
            target,
            dry_run,
            offline,
            apply_recommended,
            apply_control_flow,
            report,
        } => migrate_cmd(
            &path,
            target,
            &MigrateOptions {
                dry_run,
                offline,
                apply_recommended,
                apply_control_flow,
            },
            report,
        ),
        Commands::Catalog => print_catalog(),
    }
}

fn analyze(path: &Path) -> Result<()> {
    let project = detect::detect(path)?;
    println!("Project: {}", project.package.name());
    println!("Root:    {}", project.root.display());

    match project.angular_major() {
        Some(m) => println!("Angular: v{m}"),
        None => println!("Angular: (none found)"),
    }
    println!(
        "CLI:     {}",
        project
            .cli_major()
            .map(|m| format!("v{m}"))
            .unwrap_or_else(|| "(not installed)".into())
    );
    let all = project.package.all_dependencies();
    for key in ["typescript", "rxjs", "zone.js", "tslib"] {
        println!(
            "{key:8}: {}",
            all.get(key).map(String::as_str).unwrap_or("(none)")
        );
    }
    println!(
        "Workspace config: {}",
        project
            .workspace_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none)".into())
    );
    println!(
        "tsconfigs: {}",
        project
            .tsconfig_paths
            .iter()
            .map(|p| p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "e2e target: {}",
        if project.has_e2e_target() {
            "yes"
        } else {
            "no"
        }
    );
    println!("\nDependencies:");
    for (k, v) in all {
        println!("  {k}: {v}");
    }
    Ok(())
}

fn plan_cmd(path: &Path, target: Option<u32>, report_path: Option<PathBuf>) -> Result<()> {
    let project = detect::detect(path)?;
    let plan = plan::plan(&project, target)?;
    print!("{}", plan::render_plan(&plan, &project.root));
    if let Some(rp) = report_path {
        std::fs::write(&rp, report::plan_report(&project, &plan))?;
        println!("Plan report written to {}", rp.display());
    }
    Ok(())
}

fn migrate_cmd(
    path: &Path,
    target: Option<u32>,
    opts: &MigrateOptions,
    report_path: Option<PathBuf>,
) -> Result<()> {
    let outcome = migrate::run(path, target, opts)?;
    for line in &outcome.log {
        println!("{line}");
    }
    if !outcome.warnings.is_empty() {
        println!("\nWarnings:");
        for w in &outcome.warnings {
            println!("  ! {w}");
        }
    }
    if outcome.control_flow_skipped > 0 {
        println!(
            "\nControl-flow: {migrated} migrated, {skipped} skipped (see warnings).",
            migrated = outcome.control_flow_migrated,
            skipped = outcome.control_flow_skipped
        );
    }
    if let Some(rp) = report_path {
        let project = detect::detect(path)?;
        let plan = plan::plan(&project, target)?;
        std::fs::write(&rp, report::migrate_report(path, &outcome, &plan))?;
        println!("\nReport written to {}", rp.display());
    }
    Ok(())
}

fn print_catalog() -> Result<()> {
    println!(
        "{:>4}  {:>9}  {:>9}  {:>9}  {:>9}  {:>8}  {:>14}  {:>8}",
        "major", "core", "cli", "material", "zone.js", "rxjs", "typescript", "node"
    );
    for entry in catalog::CATALOG {
        println!(
            "{:>4}  {:>9}  {:>9}  {:>9}  {:>9}  {:>8}  {:>14}  {:>8}{}",
            entry.major,
            entry.core,
            entry.cli,
            entry.material,
            entry.zone_js,
            entry.rxjs,
            catalog::typescript_range(entry),
            entry.node,
            if entry.confirmed { "" } else { "  (est.)" }
        );
    }
    Ok(())
}

//! Project detection: locate and parse package.json, angular.json, tsconfigs.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::catalog::{catalog_major, MIN_SUPPORTED_MAJOR};
use crate::model::{PackageJson, Project};

/// Detect an Angular project rooted at `root`.
///
/// Looks for `package.json`, an optional `angular.json` / `workspace.json`,
/// and every `tsconfig*.json` at the root.
pub fn detect(root: &Path) -> Result<Project> {
    if !root.is_dir() {
        anyhow::bail!("path is not a directory: {}", root.display());
    }

    let package_path = root.join("package.json");
    if !package_path.exists() {
        anyhow::bail!(
            "no package.json found at {} (is this an Angular project?)",
            root.display()
        );
    }
    let package = read_package_json(&package_path)?;

    // angular.json (v6+) or legacy workspace.json (v6).
    let workspace_path = ["angular.json", "workspace.json"]
        .iter()
        .map(|f| root.join(f))
        .find(|p| p.exists());
    let (workspace, workspace_path) = match workspace_path {
        Some(p) => {
            let raw = std::fs::read_to_string(&p)
                .with_context(|| format!("failed to read {}", p.display()))?;
            let value = serde_json::from_str(&raw)
                .with_context(|| format!("invalid JSON in {}", p.display()))?;
            (Some(value), Some(p))
        }
        None => (None, None),
    };

    let tsconfig_paths = collect_tsconfigs(root);

    Ok(Project {
        root: root.to_path_buf(),
        package,
        workspace,
        workspace_path,
        tsconfig_paths,
    })
}

fn read_package_json(path: &Path) -> Result<PackageJson> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let data = serde_json::from_str(&raw)
        .with_context(|| format!("invalid JSON in {}", path.display()))?;
    Ok(PackageJson {
        raw,
        data,
        path: path.to_path_buf(),
    })
}

fn collect_tsconfigs(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root) {
        for entry in rd.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("tsconfig") && name.ends_with(".json") {
                paths.push(entry.path());
            }
        }
    }
    paths.sort();
    paths
}

/// Validate the detected Angular major is supported by the tool.
pub fn ensure_supported(major: u32) -> Result<()> {
    if major < MIN_SUPPORTED_MAJOR {
        anyhow::bail!(
            "Angular {major} is too old to migrate with this tool \
             (supported: Angular {MIN_SUPPORTED_MAJOR}+). \
             Consider manually upgrading to Angular {MIN_SUPPORTED_MAJOR} first."
        );
    }
    if catalog_major(major).is_none() {
        anyhow::bail!(
            "Angular {major} is newer than the latest version this tool knows \
             ({}). Update the tool before migrating.",
            crate::catalog::latest_major()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn detects_basic_project() {
        let dir = TempDir::new().unwrap();
        write(
            &dir.path().join("package.json"),
            r#"{
  "name": "demo",
  "dependencies": { "@angular/core": "^12.2.0", "rxjs": "~6.6.0" },
  "devDependencies": { "@angular/cli": "~12.2.0", "typescript": "~4.3.0" }
}"#,
        );
        write(
            &dir.path().join("angular.json"),
            r#"{ "version": 1, "projects": {} }"#,
        );
        write(&dir.path().join("tsconfig.json"), "{}");
        write(&dir.path().join("tsconfig.app.json"), "{}");

        let project = detect(dir.path()).unwrap();
        assert_eq!(project.angular_major(), Some(12));
        assert_eq!(project.cli_major(), Some(12));
        assert_eq!(project.tsconfig_paths.len(), 2);
        assert!(project.workspace.is_some());
    }

    #[test]
    fn rejects_non_project() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("README.md"), "hi");
        let err = detect(dir.path()).unwrap_err();
        assert!(err.to_string().contains("package.json"));
    }

    #[test]
    fn parses_ranges_and_aliases() {
        let dir = TempDir::new().unwrap();
        write(
            &dir.path().join("package.json"),
            r#"{
  "dependencies": { "@angular/core": ">= 15.2.0 < 16.0.0" },
  "devDependencies": { "@angular/cli": "~15.2.0" }
}"#,
        );
        let project = detect(dir.path()).unwrap();
        assert_eq!(project.angular_major(), Some(15));
    }
}

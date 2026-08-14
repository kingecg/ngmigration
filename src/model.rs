//! Core data structures shared across the tool.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

/// A parsed `package.json`.
#[derive(Debug, Clone)]
pub struct PackageJson {
    /// Original raw text (kept so untouched files are never rewritten).
    pub raw: String,
    /// Preserve-order parsed JSON.
    pub data: Value,
    /// Absolute path to the file.
    pub path: PathBuf,
}

impl PackageJson {
    pub fn name(&self) -> &str {
        self.data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unnamed project)")
    }

    /// All dependency maps (dependencies + devDependencies), merged.
    pub fn all_dependencies(&self) -> BTreeMap<String, String> {
        let mut all = BTreeMap::new();
        if let Some(d) = self.data.get("dependencies").and_then(|v| v.as_object()) {
            for (k, v) in d {
                if let Some(s) = v.as_str() {
                    all.insert(k.clone(), s.to_string());
                }
            }
        }
        if let Some(d) = self.data.get("devDependencies").and_then(|v| v.as_object()) {
            for (k, v) in d {
                if let Some(s) = v.as_str() {
                    all.insert(k.clone(), s.to_string());
                }
            }
        }
        all
    }

    pub fn dependency(&self, package: &str) -> Option<String> {
        self.all_dependencies().get(package).cloned()
    }

    pub fn scripts(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        if let Some(s) = self.data.get("scripts").and_then(|v| v.as_object()) {
            for (k, v) in s {
                if let Some(str) = v.as_str() {
                    out.insert(k.clone(), str.to_string());
                }
            }
        }
        out
    }
}

/// A fully-detected project.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub package: PackageJson,
    /// Parsed `angular.json` (or `workspace.json`), if present.
    pub workspace: Option<Value>,
    pub workspace_path: Option<PathBuf>,
    /// Every `tsconfig*.json` found at the project root.
    pub tsconfig_paths: Vec<PathBuf>,
}

impl Project {
    /// The `@angular/core` major version, if the project uses Angular.
    pub fn angular_major(&self) -> Option<u32> {
        self.package
            .dependency("@angular/core")
            .as_deref()
            .and_then(parse_major)
    }

    pub fn cli_major(&self) -> Option<u32> {
        self.package
            .dependency("@angular/cli")
            .as_deref()
            .and_then(parse_major)
    }

    /// Whether `angular.json` declares an `e2e` target on any project.
    pub fn has_e2e_target(&self) -> bool {
        let Some(w) = &self.workspace else {
            return false;
        };
        let Some(projects) = w.get("projects").and_then(|v| v.as_object()) else {
            return false;
        };
        projects.values().any(|p| {
            p.get("architect")
                .and_then(|a| a.as_object())
                .map(|a| a.contains_key("e2e"))
                .unwrap_or(false)
        })
    }
}

/// A single automated (or informational) migration operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationRule {
    /// Update an existing dependency to a version.
    DepUpdate { package: String, version: String },
    /// Remove a dependency entirely.
    DepRemove { package: String },
    /// Add a dependency (defaults to devDependencies).
    DepAdd {
        package: String,
        version: String,
        dev: bool,
    },
    /// Regex replacement over files matching a glob.
    Replace {
        glob: String,
        pattern: String,
        replacement: String,
        kind: FileKind,
    },
    /// Remove a top-level metadata field from every `@NgModule({ ... })` literal.
    RemoveNgModuleField { field: String },
    /// Remove a key from `angularCompilerOptions` in all tsconfig files.
    RemoveCompilerOption { key: String },
    /// Set a key/value inside `angularCompilerOptions`.
    SetCompilerOption { key: String, value: String },
    /// Strip deprecated `ng` CLI flags from npm scripts.
    StripScriptFlags { flags: Vec<String> },
    /// Remove a script (e.g. the `e2e` script in Angular 17).
    RemoveScript { script: String },
    /// Remove an architect target (e.g. `e2e`) from every project in angular.json.
    RemoveWorkspaceTarget { target: String },
    /// Apply the structural-directive -> control-flow rewrite (*ngIf/*ngFor).
    ControlFlowMigration,
    /// Informational only: a manual step that cannot be automated safely.
    Note { text: String },
}

impl MigrationRule {
    pub fn dep_update(p: &str, v: &str) -> Self {
        MigrationRule::DepUpdate {
            package: p.to_string(),
            version: v.to_string(),
        }
    }
    pub fn note(text: impl Into<String>) -> Self {
        MigrationRule::Note { text: text.into() }
    }
}

/// Target file kinds for regex replacements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// TypeScript sources (`.ts`), skips spec files.
    Source,
    /// HTML templates (`.html`).
    Template,
    /// Any tracked file (source + template).
    All,
}

impl FileKind {
    pub fn matches(&self, path: &str, is_spec: bool) -> bool {
        let ext_ok = path.ends_with(".ts") || path.ends_with(".html");
        if !ext_ok {
            return false;
        }
        if is_spec && matches!(self, FileKind::Source) {
            return false;
        }
        true
    }
}

/// One major-version upgrade step (e.g. 16 -> 17).
#[derive(Debug, Clone)]
pub struct MajorStep {
    pub from: u32,
    pub to: u32,
    pub rules: Vec<MigrationRule>,
}

impl MajorStep {
    pub fn notes(&self) -> Vec<&str> {
        self.rules
            .iter()
            .filter_map(|r| match r {
                MigrationRule::Note { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// A recommended version change for a third-party dependency.
#[derive(Debug, Clone)]
pub struct DependencySuggestion {
    pub package: String,
    pub from: String,
    pub to: String,
    pub reason: String,
}

/// The full, ordered migration plan from `from` to `to`.
#[derive(Debug, Clone)]
pub struct MigrationPlan {
    pub from: u32,
    pub to: u32,
    /// One step per major version traversed, in order.
    pub steps: Vec<MajorStep>,
    pub third_party: Vec<DependencySuggestion>,
}

impl MigrationPlan {
    pub fn path(&self) -> Vec<u32> {
        self.steps.iter().map(|s| s.to).collect()
    }

    pub fn notes(&self) -> Vec<&str> {
        self.steps.iter().flat_map(|s| s.notes()).collect()
    }

    /// A single flattened list of every non-note rule.
    pub fn all_actions(&self) -> Vec<&MigrationRule> {
        self.steps.iter().flat_map(|s| s.rules.iter()).collect()
    }
}

/// Parse the major version out of an npm version specifier.
///
/// Handles `^12.2.0`, `~12.1.x`, `>= 12.0.0`, `12`, `v12.0.0`, and fails
/// gracefully for ranges, git URLs, `workspace:*`, `latest`, `*`, etc.
pub fn parse_major(spec: &str) -> Option<u32> {
    let mut t = spec.trim();
    if t.is_empty() || t == "*" || t == "latest" || t == "next" {
        return None;
    }
    // Skip npm aliases / workspace links / file / git refs.
    if t.contains("git+") || t.starts_with("file:") || t.starts_with("workspace:") {
        return None;
    }
    if let Some(rest) = t.strip_prefix("npm:") {
        t = rest;
    }
    t = t.trim_start_matches(['^', '~', '>', '<', '=', 'v', ' ', '\t']);
    // npm range like ">=12.0.0 <13.0.0"
    if let Some(first) = t.split_whitespace().next() {
        t = first;
    }
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_specifiers() {
        assert_eq!(parse_major("^12.2.0"), Some(12));
        assert_eq!(parse_major("~13.1.5"), Some(13));
        assert_eq!(parse_major("14"), Some(14));
        assert_eq!(parse_major("v9.1.13"), Some(9));
        assert_eq!(parse_major(">= 12.0.0"), Some(12));
        assert_eq!(parse_major("12.1.x"), Some(12));
        assert_eq!(parse_major("*"), None);
        assert_eq!(parse_major("latest"), None);
        assert_eq!(parse_major("workspace:*"), None);
        assert_eq!(parse_major("git+https://github.com/foo/bar.git"), None);
        assert_eq!(parse_major("file:../lib"), None);
        assert_eq!(parse_major("npm:foo@^1.0.0"), None);
    }
}

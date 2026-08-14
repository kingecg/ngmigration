//! package.json manipulation: dependency rules, version alignment, scripts.

use anyhow::Result;
use regex::Regex;
use serde_json::{Map, Value};

use crate::catalog::{catalog_major, typescript_spec, MATERIAL_PACKAGES, MONOREPO_PACKAGES};
use crate::model::{MigrationRule, PackageJson};

/// Apply dependency rules (`DepUpdate`/`DepRemove`/`DepAdd`) to a package.json.
/// Returns the number of applied changes.
pub fn apply_dep_rules(pkg: &mut PackageJson, rules: &[MigrationRule]) -> usize {
    let mut changes = 0;
    for rule in rules {
        match rule {
            MigrationRule::DepUpdate { package, version } => {
                if update_in_place(pkg, package, version) {
                    changes += 1;
                }
            }
            MigrationRule::DepRemove { package } => {
                if remove_anywhere(pkg, package) {
                    changes += 1;
                }
            }
            MigrationRule::DepAdd {
                package,
                version,
                dev,
            } => {
                let key = if *dev {
                    "devDependencies"
                } else {
                    "dependencies"
                };
                if add_or_update(pkg, key, package, version) {
                    changes += 1;
                }
            }
            _ => {}
        }
    }
    changes
}

fn get_section_mut<'a>(pkg: &'a mut PackageJson, key: &str) -> Option<&'a mut Map<String, Value>> {
    pkg.data.get_mut(key).and_then(|v| v.as_object_mut())
}

/// Update an existing dependency wherever it appears (deps first, then dev).
fn update_in_place(pkg: &mut PackageJson, package: &str, version: &str) -> bool {
    if update_section(pkg, "dependencies", package, version) {
        return true;
    }
    update_section(pkg, "devDependencies", package, version)
}

fn update_section(pkg: &mut PackageJson, section: &str, package: &str, version: &str) -> bool {
    let Some(map) = get_section_mut(pkg, section) else {
        return false;
    };
    let Some(existing) = map.get(package) else {
        return false;
    };
    if existing.as_str() == Some(version) {
        return false;
    }
    map.insert(package.to_string(), Value::String(version.to_string()));
    true
}

fn remove_anywhere(pkg: &mut PackageJson, package: &str) -> bool {
    let mut changed = false;
    for section in ["dependencies", "devDependencies"] {
        if let Some(map) = get_section_mut(pkg, section) {
            if map.remove(package).is_some() {
                changed = true;
            }
        }
    }
    changed
}

fn add_or_update(pkg: &mut PackageJson, section: &str, package: &str, version: &str) -> bool {
    if let Some(map) = get_section_mut(pkg, section) {
        match map.get(package) {
            Some(existing) if existing.as_str() == Some(version) => return false,
            Some(_) => {
                map.insert(package.to_string(), Value::String(version.to_string()));
                return true;
            }
            None => {
                map.insert(package.to_string(), Value::String(version.to_string()));
                return true;
            }
        }
    }
    pkg.data[section] = Value::Object(Map::new());
    pkg.data[section][package] = Value::String(version.to_string());
    true
}

/// Align every managed dependency to the catalog versions of `target_major`.
/// Returns the number of version changes made.
pub fn align_dependencies(pkg: &mut PackageJson, target_major: u32) -> usize {
    let Some(cat) = catalog_major(target_major) else {
        return 0;
    };
    let mut changes = 0;
    for section in ["dependencies", "devDependencies"] {
        let Some(map) = get_section_mut(pkg, section) else {
            continue;
        };
        let keys: Vec<String> = map.keys().cloned().collect();
        for key in keys {
            let spec: Option<String> = if MONOREPO_PACKAGES.contains(&key.as_str()) {
                Some(format!("^{}", cat.core))
            } else if key == "@angular/cli" {
                Some(format!("^{}", cat.cli))
            } else if MATERIAL_PACKAGES.contains(&key.as_str()) {
                Some(format!("^{}", cat.material))
            } else if key.starts_with("@angular-devkit/") {
                Some(format!("^{}", cat.cli))
            } else {
                match key.as_str() {
                    "rxjs" => Some(format!("^{}", cat.rxjs)),
                    "zone.js" => Some(format!("^{}", cat.zone_js)),
                    "typescript" => Some(typescript_spec(cat)),
                    "tslib" => Some(format!("^{}", cat.tslib)),
                    _ => None,
                }
            };
            if let Some(spec) = spec {
                if map.get(&key).and_then(|v| v.as_str()) == Some(spec.as_str()) {
                    continue;
                }
                map.insert(key, Value::String(spec));
                changes += 1;
            }
        }
    }
    changes
}

/// Apply script rules (`StripScriptFlags`, `RemoveScript`).
pub fn apply_script_rules(pkg: &mut PackageJson, rules: &[MigrationRule]) -> usize {
    let mut changes = 0;
    for rule in rules {
        match rule {
            MigrationRule::StripScriptFlags { flags } => {
                let Some(scripts) = get_section_mut(pkg, "scripts") else {
                    continue;
                };
                for (_name, val) in scripts.iter_mut() {
                    let Some(s) = val.as_str() else {
                        continue;
                    };
                    if !s.contains("ng ") && !s.starts_with("ng ") {
                        continue;
                    }
                    let next = strip_flags(s, flags);
                    if next != s {
                        *val = Value::String(next);
                        changes += 1;
                    }
                }
            }
            MigrationRule::RemoveScript { script } => {
                if let Some(scripts) = get_section_mut(pkg, "scripts") {
                    if scripts.remove(script).is_some() {
                        changes += 1;
                    }
                }
            }
            _ => {}
        }
    }
    changes
}

fn strip_flags(script: &str, flags: &[String]) -> String {
    let mut out = script.to_string();
    for flag in flags {
        // `flags` entries include the leading `--` (e.g. `--prod`).
        let pat = format!(r"\s+{}\b", regex::escape(flag));
        if let Ok(re) = Regex::new(&pat) {
            out = re.replace_all(&out, "").into_owned();
        }
    }
    out
}

/// Serialize the package.json back to disk with 2-space indentation.
pub fn save(pkg: &PackageJson, dry: bool) -> Result<()> {
    let pretty = serde_json::to_string_pretty(&pkg.data)?;
    let rendered = format!("{pretty}\n");
    if !dry {
        std::fs::write(&pkg.path, rendered)?;
    }
    Ok(())
}

/// Apply a set of dependency suggestions (from third-party resolution).
pub fn apply_suggestions(
    pkg: &mut PackageJson,
    suggestions: &[crate::model::DependencySuggestion],
) -> usize {
    let mut changes = 0;
    for s in suggestions {
        if update_in_place(pkg, &s.package, &s.to) {
            changes += 1;
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn pkg_from(content: &str) -> PackageJson {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("am-test-pkg-{}-{id}.json", std::process::id()));
        std::fs::write(&path, content).unwrap();
        let data = serde_json::from_str(content).unwrap();
        PackageJson {
            raw: content.to_string(),
            data,
            path,
        }
    }

    #[test]
    fn applies_dep_update_and_remove() {
        let mut pkg = pkg_from(
            r#"{ "dependencies": { "@angular/core": "^12.2.0", "protractor": "^7.0.0" } }"#,
        );
        let rules = vec![
            MigrationRule::DepRemove {
                package: "protractor".into(),
            },
            MigrationRule::DepUpdate {
                package: "@angular/core".into(),
                version: "^13.0.0".into(),
            },
        ];
        assert_eq!(apply_dep_rules(&mut pkg, &rules), 2);
        assert!(pkg.dependency("protractor").is_none());
        assert_eq!(pkg.dependency("@angular/core").as_deref(), Some("^13.0.0"));
    }

    #[test]
    fn aligns_monorepo_and_toolchain() {
        let mut pkg = pkg_from(
            r#"{
  "dependencies": {
    "@angular/core": "^16.2.0",
    "@angular/common": "^16.2.0",
    "@angular/router": "^16.2.0",
    "@angular/material": "^16.2.0",
    "rxjs": "~7.5.0"
  },
  "devDependencies": {
    "@angular/cli": "^16.2.0",
    "typescript": "~4.9.0",
    "zone.js": "~0.14.0",
    "@angular-devkit/build-angular": "^16.2.0"
  }
}"#,
        );
        let changes = align_dependencies(&mut pkg, 19);
        assert!(changes >= 9);
        assert_eq!(pkg.dependency("@angular/core").as_deref(), Some("^19.2.25"));
        assert_eq!(
            pkg.dependency("@angular/material").as_deref(),
            Some("^19.2.19")
        );
        assert_eq!(pkg.dependency("rxjs").as_deref(), Some("^7.8.1"));
        assert_eq!(pkg.dependency("zone.js").as_deref(), Some("^0.15.1"));
        let ts = pkg.data["devDependencies"]["typescript"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(ts, "~5.8.0");
    }

    #[test]
    fn strips_deprecated_flags() {
        let mut pkg =
            pkg_from(r#"{ "scripts": { "build": "ng build --prod", "serve": "ng serve" } }"#);
        let rules = vec![MigrationRule::StripScriptFlags {
            flags: vec!["--prod".into()],
        }];
        assert_eq!(apply_script_rules(&mut pkg, &rules), 1);
        assert_eq!(pkg.scripts()["build"], "ng build");
        assert_eq!(pkg.scripts()["serve"], "ng serve");
    }

    #[test]
    fn saves_pretty_json() {
        let mut pkg = pkg_from(r#"{ "name": "x", "dependencies": {} }"#);
        align_dependencies(&mut pkg, 17);
        save(&pkg, false).unwrap();
        let reread = std::fs::read_to_string(&pkg.path).unwrap();
        assert!(reread.contains("\n  \"name\": \"x\","));
        assert!(reread.trim_end().ends_with('}'));
    }
}

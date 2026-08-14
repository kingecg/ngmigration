//! tsconfig.json handling: JSONC-aware read/modify/write of
//! `angularCompilerOptions`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Map, Value};

use crate::model::Project;

fn next_significant(b: &[u8], mut i: usize) -> Option<u8> {
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        return b.get(i).copied();
    }
}

/// Strip JSONC comments and trailing commas so content can be parsed as JSON.
/// String literals and template literals are respected.
pub fn strip_jsonc(input: &str) -> String {
    let b = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let mut in_string = false;
    let mut quote = b' ';
    while i < b.len() {
        let c = b[i];
        if in_string {
            out.push(c as char);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1] as char);
                i += 2;
                continue;
            }
            if c == quote {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => {
                in_string = true;
                quote = c;
                out.push(c as char);
                i += 1;
            }
            b'/' if i + 1 < b.len() && b[i + 1] == b'/' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < b.len() && b[i + 1] == b'*' => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b',' => {
                if matches!(next_significant(b, i + 1), Some(b'}') | Some(b']')) {
                    i += 1; // trailing comma: drop it
                } else {
                    out.push(',');
                    i += 1;
                }
            }
            _ => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

/// Read a tsconfig, returning its parsed JSON (JSONC tolerated).
pub fn read_tsconfig(path: &Path) -> Result<Value> {
    let raw = std::fs::read_to_string(path)?;
    match serde_json::from_str(&raw) {
        Ok(v) => Ok(v),
        Err(_) => {
            let cleaned = strip_jsonc(&raw);
            serde_json::from_str(&cleaned)
                .map_err(|e| anyhow::anyhow!("cannot parse {} as JSON/JSONC: {e}", path.display()))
        }
    }
}

fn angular_compiler_options(root: &mut Map<String, Value>) -> Option<&mut Map<String, Value>> {
    root.get_mut("angularCompilerOptions")
        .and_then(|v| v.as_object_mut())
}

/// Set/remove keys inside `angularCompilerOptions` for every tsconfig in the
/// project. Returns the list of files that changed.
pub fn apply_compiler_options(
    project: &Project,
    removes: &[String],
    sets: &BTreeMap<String, String>,
    dry: bool,
) -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    for path in &project.tsconfig_paths {
        let raw = std::fs::read_to_string(path)?;
        let mut value = read_tsconfig(path)?;
        let Some(obj) = value.as_object_mut() else {
            continue;
        };
        let Some(aco) = angular_compiler_options(obj) else {
            continue;
        };
        let mut dirty = false;
        for key in removes {
            if aco.remove(key).is_some() {
                dirty = true;
            }
        }
        for (key, val) in sets {
            let parsed: Value = if let Ok(n) = val.parse::<i64>() {
                Value::from(n)
            } else if let Ok(f) = val.parse::<f64>() {
                Value::from(f)
            } else {
                Value::String(val.clone())
            };
            if aco.get(key) != Some(&parsed) {
                aco.insert(key.clone(), parsed);
                dirty = true;
            }
        }
        if dirty {
            let next = serde_json::to_string_pretty(&value)? + "\n";
            if next != raw {
                if !dry {
                    std::fs::write(path, next)?;
                }
                changed.push(path.clone());
            }
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_and_trailing_commas() {
        let input = r#"{
  // base config
  "extends": "./tsconfig.json",
  "compilerOptions": { "strict": true, },
  "angularCompilerOptions": { "enableIvy": true, /* legacy */ },
}"#;
        let cleaned = strip_jsonc(input);
        let v: Value = serde_json::from_str(&cleaned).unwrap();
        assert_eq!(v["compilerOptions"]["strict"], Value::Bool(true));
    }

    #[test]
    fn strip_keeps_strings_with_slashes() {
        let input = r#"{ "a": "http://x/y", "b": "/* not a comment */" }"#;
        let cleaned = strip_jsonc(input);
        let v: Value = serde_json::from_str(&cleaned).unwrap();
        assert_eq!(v["a"], "http://x/y");
        assert_eq!(v["b"], "/* not a comment */");
    }
}

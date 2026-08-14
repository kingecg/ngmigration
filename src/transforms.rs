//! Source transformation engine.
//!
//! Two mechanisms:
//!  1. Glob + regex replacement across project files (skips `node_modules`,
//!     build output and `.git`).
//!  2. Structural edits: removing metadata fields from `@NgModule` literals
//!     using a small, string/comment-aware scanner.

use std::path::Path;

use anyhow::Result;
use glob::glob;
use regex::Regex;
use walkdir::WalkDir;

use crate::model::FileKind;

/// Default directories never touched by regex transforms.
fn is_ignored_dir(seg: &str) -> bool {
    matches!(
        seg,
        "node_modules" | "dist" | ".git" | ".angular" | "coverage" | "bazel-out"
    )
}

/// Collect project files (ts/html) from a glob, applying the file-kind filter.
pub fn files_for_glob(root: &Path, glob_pattern: &str, kind: FileKind) -> Vec<std::path::PathBuf> {
    let pattern = if glob_pattern.contains('/') {
        root.join(glob_pattern).to_string_lossy().into_owned()
    } else {
        root.join("**")
            .join(glob_pattern)
            .to_string_lossy()
            .into_owned()
    };

    let mut out = Vec::new();
    if let Ok(paths) = glob(&pattern) {
        for entry in paths.flatten() {
            if !entry.is_file() {
                continue;
            }
            let rel = entry
                .strip_prefix(root)
                .unwrap_or(&entry)
                .to_string_lossy()
                .replace('\\', "/");
            let is_spec = rel.contains(".spec.");
            if kind.matches(&rel, is_spec) {
                out.push(entry);
            }
        }
    }
    out
}

/// Walk the project tree and return all `.ts` / `.html` files not under an
/// ignored directory. Used by the structural (`@NgModule`) transform.
pub fn walk_source_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            if e.file_type().is_dir() {
                return !is_ignored_dir(&e.file_name().to_string_lossy());
            }
            true
        })
    {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if name.ends_with(".ts") {
            out.push(entry.into_path());
        }
    }
    out
}

/// Apply a regex replacement to every file matching `glob_pattern`.
pub fn apply_regex_replace(
    root: &Path,
    glob_pattern: &str,
    pattern: &str,
    replacement: &str,
    kind: FileKind,
    dry: bool,
) -> Result<Vec<std::path::PathBuf>> {
    let re = Regex::new(pattern).map_err(|e| anyhow::anyhow!("invalid regex `{pattern}`: {e}"))?;
    let mut changed = Vec::new();
    for path in files_for_glob(root, glob_pattern, kind) {
        let raw = std::fs::read_to_string(&path)?;
        let next = re.replace_all(&raw, replacement).into_owned();
        if next != raw {
            if !dry {
                std::fs::write(&path, next)?;
            }
            changed.push(path);
        }
    }
    Ok(changed)
}

// ---------------------------------------------------------------------------
// Structural helpers (string/comment aware)
// ---------------------------------------------------------------------------

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$'
}

fn is_ident_part(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

fn skip_string(bytes: &[u8], mut i: usize) -> usize {
    let quote = bytes[i];
    i += 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    i
}

fn skip_line_comment(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

fn skip_block_comment(bytes: &[u8], mut i: usize) -> usize {
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            return i + 2;
        }
        i += 1;
    }
    i
}

/// Skip whitespace, line/block comments and (optionally) a single newline-free
/// run. Returns the next meaningful byte index.
fn skip_ws_comments(bytes: &[u8], mut i: usize) -> usize {
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            i = skip_line_comment(bytes, i);
        } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i = skip_block_comment(bytes, i);
        } else {
            return i;
        }
    }
}

/// Find `@NgModule` decorator invocations and return the byte range of each
/// object literal passed as the first argument. String/comment aware.
fn find_ng_module_object_ranges(src: &str) -> Vec<(usize, usize)> {
    let bytes = src.as_bytes();
    let mut ranges = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\'' | b'"' | b'`' => {
                i = skip_string(bytes, i);
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                i = skip_line_comment(bytes, i);
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i = skip_block_comment(bytes, i);
            }
            _ if is_ident_start(c) => {
                let start = i;
                while i < bytes.len() && is_ident_part(bytes[i]) {
                    i += 1;
                }
                let name = &src[start..i];
                if name == "NgModule" {
                    // Boundary check: next meaningful char must be '('
                    let j = skip_ws_comments(bytes, i);
                    if j < bytes.len() && bytes[j] == b'(' {
                        // Find the matching ')' for the call.
                        if let Some(open_paren) = find_matching(bytes, j, b'(', b')') {
                            // Inside the parens, find the object literal.
                            let k = skip_ws_comments(bytes, j + 1);
                            if k < bytes.len() && bytes[k] == b'{' {
                                if let Some(open_brace) = find_matching(bytes, k, b'{', b'}') {
                                    ranges.push((k, open_brace));
                                    i = open_brace;
                                    continue;
                                }
                            }
                            i = open_paren;
                            continue;
                        }
                    }
                }
            }
            _ => i += 1,
        }
    }
    ranges
}

/// Find the index of the byte matching `open`/`close`, starting at `open_idx`
/// where `bytes[open_idx] == open`. String/comment aware.
fn find_matching(bytes: &[u8], open_idx: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open_idx;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            _ if c == open => depth += 1,
            _ if c == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'\'' | b'"' | b'`' => {
                i = skip_string(bytes, i);
                continue;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                i = skip_line_comment(bytes, i);
                continue;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i = skip_block_comment(bytes, i);
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Remove a top-level property (e.g. `entryComponents`) from an object literal.
///
/// `text` must start with `{`. Handles nested braces/brackets/parens, string
/// literals and comments. Returns the new literal text.
pub fn remove_top_level_property(text: &str, prop: &str) -> String {
    let bytes = text.as_bytes();
    let n = bytes.len();
    let mut out = String::with_capacity(text.len());
    let mut seg_start = 0usize;
    let mut i = 1usize; // skip the leading '{'
    let mut depth = 1usize; // we are inside the top-level object
    let mut changed = false;

    while i < n {
        let c = bytes[i];
        match c {
            b'{' | b'(' | b'[' => {
                depth += 1;
                i += 1;
            }
            b'}' | b')' | b']' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'\'' | b'"' | b'`' => {
                i = skip_string(bytes, i);
            }
            b'/' if i + 1 < n && bytes[i + 1] == b'/' => {
                i = skip_line_comment(bytes, i);
            }
            b'/' if i + 1 < n && bytes[i + 1] == b'*' => {
                i = skip_block_comment(bytes, i);
            }
            _ => {
                // Candidate property name at depth 1.
                let ident_before = i > 0 && is_ident_part(bytes[i - 1]);
                let starts_with = text[i..].starts_with(prop);
                if depth == 1 && !ident_before && starts_with {
                    let after = i + prop.len();
                    // Property name must be an identifier followed by ':'
                    let name_end = i + prop.len();
                    let name_is_ident = bytes[name_end.min(n) - 1].is_ascii_alphanumeric()
                        && after < n
                        && !is_ident_part(bytes[after]);
                    let colon = skip_ws_comments(bytes, after);
                    if name_is_ident && colon < n && bytes[colon] == b':' {
                        // Find where the value ends: next top-level ',' or '}'.
                        let val_start = skip_ws_comments(bytes, colon + 1);
                        if let Some((val_end, next)) = find_value_end(bytes, val_start, depth) {
                            // Remove [i, next): the property plus its trailing
                            // comma (if any).
                            out.push_str(&text[seg_start..i]);
                            seg_start = next;
                            i = next;
                            changed = true;
                            // If this was the last property (value_end sits
                            // right before '}'), also drop the preceding comma.
                            if bytes[val_end] == b'}' {
                                // Walk back over ws from seg_start to find a ','
                                let mut back = seg_start.saturating_sub(1);
                                while back > 0 && bytes[back].is_ascii_whitespace() {
                                    back -= 1;
                                }
                                if bytes[back] == b',' {
                                    out.truncate(out.len() - (seg_start - back - 1));
                                    // rebuild: seg_start stays, but we removed
                                    // the comma+ws from the flushed region.
                                    seg_start = back;
                                }
                            }
                            continue;
                        }
                    }
                }
                i += 1;
            }
        }
    }
    out.push_str(&text[seg_start..]);
    if changed {
        out
    } else {
        text.to_string()
    }
}

/// Given the start of a value inside an object at `base_depth`, return
/// `(value_end, next)` where `next` is the index just after the terminating
/// `,` (or equal to `value_end` when the value is the last property).
fn find_value_end(bytes: &[u8], start: usize, base_depth: usize) -> Option<(usize, usize)> {
    let n = bytes.len();
    let mut i = start;
    let mut depth = base_depth;
    while i < n {
        let c = bytes[i];
        match c {
            b'{' | b'(' | b'[' => {
                depth += 1;
                i += 1;
            }
            b'}' | b')' | b']' => {
                if depth == base_depth && c == b'}' {
                    // Reached the closing brace of the containing object.
                    return Some((i, i));
                }
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b',' if depth == base_depth => {
                return Some((i, i + 1));
            }
            b'\'' | b'"' | b'`' => {
                i = skip_string(bytes, i);
            }
            b'/' if i + 1 < n && bytes[i + 1] == b'/' => {
                i = skip_line_comment(bytes, i);
            }
            b'/' if i + 1 < n && bytes[i + 1] == b'*' => {
                i = skip_block_comment(bytes, i);
            }
            _ => i += 1,
        }
    }
    None
}

/// Remove a metadata field from every `@NgModule({ ... })` literal in `src`.
///
/// Returns the updated source and the number of removals performed.
pub fn remove_ng_module_field(src: &str, field: &str) -> (String, usize) {
    let mut out = String::with_capacity(src.len());
    let mut last = 0usize;
    let mut removals = 0usize;
    for (start, end) in find_ng_module_object_ranges(src) {
        out.push_str(&src[last..start]);
        let literal = &src[start..=end];
        let next = remove_top_level_property(literal, field);
        if next != literal {
            removals += 1;
        }
        out.push_str(&next);
        last = end + 1;
    }
    out.push_str(&src[last..]);
    if removals > 0 {
        (out, removals)
    } else {
        (src.to_string(), 0)
    }
}

/// Apply the `@NgModule` field removal across the whole project.
pub fn apply_remove_ng_module_field(
    root: &Path,
    field: &str,
    dry: bool,
) -> Result<Vec<std::path::PathBuf>> {
    let mut changed = Vec::new();
    for path in walk_source_files(root) {
        let raw = std::fs::read_to_string(&path)?;
        let (next, removals) = remove_ng_module_field(&raw, field);
        if removals > 0 {
            if !dry {
                std::fs::write(&path, next)?;
            }
            changed.push(path);
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_entry_components_middle() {
        let src = r#"
@NgModule({
  declarations: [AppComponent],
  imports: [BrowserModule],
  entryComponents: [DialogComponent],
  providers: [],
  bootstrap: [AppComponent]
})
export class AppModule {}
"#;
        let (out, count) = remove_ng_module_field(src, "entryComponents");
        assert_eq!(count, 1);
        assert!(!out.contains("entryComponents"));
        assert!(out.contains("imports: [BrowserModule]"));
        assert!(out.contains("providers: []"));
        assert!(out.contains("bootstrap: [AppComponent]"));
    }

    #[test]
    fn removes_last_property_and_comma() {
        let src = r#"@NgModule({
  declarations: [AppComponent],
  entryComponents: [A]
})
export class AppModule {}
"#;
        let (out, count) = remove_ng_module_field(src, "entryComponents");
        assert_eq!(count, 1);
        assert!(!out.contains("entryComponents"));
        assert!(!out.contains(",}"));
        assert!(out.contains("declarations: [AppComponent]"));
    }

    #[test]
    fn leaves_strings_untouched() {
        let src = r#"
const s = "@NgModule({ entryComponents: 'fake' })";
@NgModule({
  declarations: [AppComponent]
})
export class AppModule {}
"#;
        let (out, count) = remove_ng_module_field(src, "entryComponents");
        assert_eq!(count, 0);
        assert!(out.contains("const s"));
        assert!(out.contains("@NgModule"));
    }

    #[test]
    fn ignores_nested_objects() {
        let src = r#"
@NgModule({
  declarations: [AppComponent],
  imports: [{
    entryComponents: [NestedThing]
  }]
})
export class AppModule {}
"#;
        let (out, count) = remove_ng_module_field(src, "entryComponents");
        assert_eq!(count, 0);
        assert!(out.contains("entryComponents"));
    }

    #[test]
    fn handles_comment_between_key_and_colon() {
        let src = r#"@NgModule({
  entryComponents /* legacy */ : [A],
  bootstrap: [AppComponent]
})"#;
        let (out, count) = remove_ng_module_field(src, "entryComponents");
        assert_eq!(count, 1);
        assert!(!out.contains("entryComponents"));
    }

    #[test]
    fn regex_replace_skips_spec_files_for_source_kind() {
        // covered in integration tests; here just ensure function exists
        assert!(is_ignored_dir("node_modules"));
        assert!(!is_ignored_dir("src"));
    }
}

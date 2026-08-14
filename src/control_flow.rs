//! Opt-in migration from structural directives (`*ngIf`, `*ngFor`) to the
//! modern control-flow blocks (`@if`, `@for`) introduced in Angular 17.
//!
//! This is deliberately conservative:
//!   * only `.html` templates are rewritten (inline templates are left alone);
//!   * `*ngFor` loops that use an index variable are skipped (index must become
//!     `$index`, which cannot be rewritten safely);
//!   * `ng-template` elements and `*ngIf`/`*ngFor` with aliases are skipped;
//!   * a warning is emitted for every skipped occurrence.

use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::Result;
use regex::Regex;

/// Per-run outcome.
#[derive(Debug, Default)]
pub struct ControlFlowResult {
    pub changed: Vec<PathBuf>,
    pub migrated: usize,
    pub skipped: usize,
    pub warnings: Vec<String>,
}

/// An HTML attribute.
#[derive(Debug)]
struct Attr {
    name: String,
    /// Raw value text *including* surrounding quotes (or `None` for boolean).
    value: Option<String>,
}

/// A parsed element (open tag + matching close tag, if any).
#[derive(Debug)]
struct Element {
    name: String,
    open_range: Range<usize>,
    self_closing: bool,
    attrs: Vec<Attr>,
    /// Byte range of element content (empty for self-closing).
    content_range: Range<usize>,
    close_end: usize,
    /// True when the element had a matching close tag (not self-closing).
    has_close: bool,
}

impl Element {
    fn attr(&self, name: &str) -> Option<&Attr> {
        self.attrs.iter().find(|a| a.name == name)
    }
}

const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

fn is_void(name: &str) -> bool {
    VOID_ELEMENTS.contains(&name)
}

fn find_next_lt(src: &str, from: usize, end: usize) -> Option<usize> {
    (from..end).find(|&i| src.as_bytes()[i] == b'<')
}

/// Parse the open tag starting at `lt` (where `src[lt] == '<'`).
fn parse_open_tag(src: &str, lt: usize, end: usize) -> Option<(Element, usize)> {
    let b = src.as_bytes();
    let n = b.len().min(end);
    let mut i = lt + 1;
    // Comments / doctypes are not elements.
    if i < n && (b[i] == b'!' || b[i] == b'?') {
        return None;
    }
    let name_start = i;
    while i < n && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'-' | b':' | b'_')) {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name = src[name_start..i].to_string();

    let mut attrs: Vec<Attr> = Vec::new();
    let mut self_closing = false;
    let mut ended = false;
    let mut open_end = 0usize;

    while i < n {
        while i < n && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= n {
            break;
        }
        let c = b[i];
        if c == b'>' {
            open_end = i + 1;
            ended = true;
            break;
        }
        if c == b'/' && i + 1 < n && b[i + 1] == b'>' {
            self_closing = true;
            open_end = i + 2;
            ended = true;
            break;
        }
        // Attribute name (structural/binding/ref/event chars allowed).
        let a_start = i;
        while i < n
            && (b[i].is_ascii_alphanumeric()
                || matches!(
                    b[i],
                    b'-' | b':' | b'_' | b'*' | b'#' | b'[' | b']' | b'(' | b')' | b'.'
                ))
        {
            i += 1;
        }
        if i == a_start {
            i += 1; // unknown char; skip
            continue;
        }
        let a_name = src[a_start..i].to_string();
        // Skip whitespace; look for '='.
        let mut j = i;
        while j < n && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let mut value = None;
        let a_end;
        if j < n && b[j] == b'=' {
            j += 1;
            while j < n && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < n {
                let v_start = j;
                if b[j] == b'"' || b[j] == b'\'' {
                    let q = b[j];
                    j += 1;
                    while j < n && b[j] != q {
                        j += 1;
                    }
                    j = (j + 1).min(n);
                } else {
                    while j < n && !b[j].is_ascii_whitespace() && b[j] != b'>' && b[j] != b'/' {
                        j += 1;
                    }
                }
                value = Some(src[v_start..j].to_string());
            }
            a_end = j;
        } else {
            a_end = i;
        }
        attrs.push(Attr {
            name: a_name,
            value,
        });
        i = a_end;
    }

    if !ended {
        return None; // malformed tag, no '>' found
    }

    let void = is_void(&name);
    Some((
        Element {
            name: name.clone(),
            open_range: lt..open_end,
            self_closing: self_closing || void,
            attrs,
            content_range: open_end..open_end,
            close_end: open_end,
            has_close: false,
        },
        open_end,
    ))
}

/// Locate the matching close tag `</name>` starting the scan after
/// `open_end`. Returns the index just past the close tag.
///
/// Uses a per-name counter: nested elements with the same tag name are
/// balanced against their own close tags, so only the outer element's
/// closing tag (count back to zero) is returned.
fn find_close_tag(src: &str, name: &str, from: usize, end: usize) -> Option<usize> {
    let b = src.as_bytes();
    let n = b.len().min(end);
    let mut depth = 1usize; // the element itself
    let mut i = from;
    while i < n {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        if i + 1 < n && b[i + 1] == b'!' {
            // comment or doctype: skip to '>'
            let mut j = i + 2;
            while j < n && b[j] != b'>' {
                j += 1;
            }
            i = (j + 1).min(n);
            continue;
        }
        if i + 1 < n && b[i + 1] == b'/' {
            // closing tag
            let mut j = i + 2;
            while j < n && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'-' | b':' | b'_')) {
                j += 1;
            }
            let cname = &src[i + 2..j];
            if cname == name {
                depth -= 1;
                if depth == 0 {
                    while j < n && b[j] != b'>' {
                        j += 1;
                    }
                    return Some((j + 1).min(n));
                }
            }
            i = j;
            continue;
        }
        // open tag inside
        if i + 1 < n && (b[i + 1].is_ascii_alphabetic() || b[i + 1] == b'_') {
            let mut j = i + 1;
            while j < n && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'-' | b':' | b'_')) {
                j += 1;
            }
            let oname = &src[i + 1..j];
            if oname == name {
                depth += 1;
            }
            // skip to '>' respecting quotes
            let mut k = j;
            let mut in_q: Option<u8> = None;
            while k < n {
                let c = b[k];
                if let Some(q) = in_q {
                    if c == b'\\' {
                        k += 2;
                        continue;
                    }
                    if c == q {
                        in_q = None;
                    }
                    k += 1;
                    continue;
                }
                if c == b'"' || c == b'\'' {
                    in_q = Some(c);
                } else if c == b'>' {
                    break;
                }
                k += 1;
            }
            i = (k + 1).min(n);
            continue;
        }
        i += 1;
    }
    None
}

/// Fully parse an element starting at `lt`.
fn parse_element(src: &str, lt: usize, end: usize) -> Option<Element> {
    let (mut el, open_end) = parse_open_tag(src, lt, end)?;
    if el.self_closing {
        el.content_range = open_end..open_end;
        el.close_end = open_end;
        return Some(el);
    }
    let close_end = find_close_tag(src, &el.name, open_end, end)?;
    el.content_range = open_end
        ..close_end
            .saturating_sub(el.name.len() + 3)
            .max(open_end)
            .min(close_end);
    // content range: between open_end and the '<' of '</name>'
    let close_start = close_end.saturating_sub(el.name.len() + 3);
    el.content_range = open_end..close_start;
    el.close_end = close_end;
    el.has_close = true;
    Some(el)
}

/// Parse a `*ngFor` expression.
#[derive(Debug, Clone)]
struct ForExpr {
    item: String,
    iterable: String,
    index_var: Option<String>,
    track_by: Option<String>,
}

fn parse_for_expr(expr: &str) -> Option<ForExpr> {
    let parts: Vec<&str> = expr.split(';').collect();
    let head = parts[0].trim();
    if !head.starts_with("let ") {
        return None;
    }
    let body = head[4..].trim();
    let (item, iterable) = body.split_once(" of ")?;
    let item = item.trim();
    let iterable = iterable.trim();
    if item.contains(',') || item.contains(' ') || iterable.contains(" as ") {
        return None; // old-school aliases: skip
    }
    let mut index_var = None;
    let mut track_by = None;
    for part in &parts[1..] {
        let p = part.trim();
        if let Some(rest) = p.strip_prefix("let ") {
            let (var, val) = rest.split_once('=').map(|(a, b)| (a.trim(), b.trim()))?;
            if val == "index" {
                index_var = Some(var.to_string());
            } else {
                return None; // unknown micro-syntax
            }
        } else if let Some(rest) = p.strip_prefix("trackBy:") {
            track_by = Some(rest.trim().to_string());
        } else if !p.is_empty() {
            return None; // unknown micro-syntax
        }
    }
    Some(ForExpr {
        item: item.to_string(),
        iterable: iterable.to_string(),
        index_var,
        track_by,
    })
}

fn leading_ws_of_line(src: &str, pos: usize) -> String {
    let line_start = src[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line = &src[line_start..pos];
    line.chars()
        .take_while(|c| c.is_whitespace() && *c != '\n')
        .collect()
}

/// Re-indent every non-empty line of `text` by prepending `indent`.
fn reindent(text: &str, indent: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    text.lines()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                format!("{indent}{l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Decide what to do with an element.
enum Decision {
    MigrateIf(String),
    MigrateFor(ForExpr),
    Keep,
    Skip(&'static str),
}

fn decide(el: &Element) -> Decision {
    let ngi = el.attr("*ngIf");
    let ngf = el.attr("*ngFor");
    match (ngi, ngf) {
        (Some(a), None) => {
            if el.attr("[ngIfElse]").is_some() || el.attr("[ngIfThen]").is_some() {
                return Decision::Skip("ngIf with else/then branches cannot be migrated safely");
            }
            let expr = a
                .value
                .as_deref()
                .map(|v| v.trim_matches('"').trim_matches('\''));
            let Some(expr) = expr else {
                return Decision::Skip("malformed *ngIf expression");
            };
            if expr.is_empty() {
                return Decision::Skip("empty *ngIf expression");
            }
            if expr.contains(';') || expr.contains(" as ") {
                return Decision::Skip(
                    "ngIf uses micro-syntax (else/then/alias); migrate manually",
                );
            }
            Decision::MigrateIf(expr.to_string())
        }
        (None, Some(a)) => {
            if el.name == "ng-template" {
                return Decision::Skip("ng-template [ngForOf] requires manual migration");
            }
            let raw = a.value.as_deref().unwrap_or("");
            let raw = raw.trim_matches('"').trim_matches('\'');
            match parse_for_expr(raw) {
                Some(f) if f.index_var.is_some() => {
                    Decision::Skip("ngFor uses an index variable; index becomes $index and must be updated manually")
                }
                Some(f) => Decision::MigrateFor(f),
                None => Decision::Skip("unsupported *ngFor micro-syntax"),
            }
        }
        _ => Decision::Keep,
    }
}

/// Rebuild the open tag without the given attribute names.
fn rebuild_open_tag(el: &Element, drop: &[&str]) -> String {
    let mut out = format!("<{}", el.name);
    for a in &el.attrs {
        if drop.contains(&a.name.as_str()) {
            continue;
        }
        out.push(' ');
        out.push_str(&a.name);
        if let Some(v) = &a.value {
            out.push('=');
            out.push_str(v);
        }
    }
    if el.self_closing {
        out.push_str(" />");
    } else {
        out.push('>');
    }
    out
}

struct Stats {
    migrated: usize,
    skipped: usize,
    warnings: Vec<String>,
}

fn process_range(src: &str, range: Range<usize>, stats: &mut Stats) -> String {
    let mut out = String::new();
    let mut cursor = range.start;
    while cursor < range.end {
        match find_next_lt(src, cursor, range.end) {
            Some(lt) => {
                out.push_str(&src[cursor..lt]);
                if let Some(el) = parse_element(src, lt, range.end) {
                    match decide(&el) {
                        Decision::MigrateIf(expr) => {
                            stats.migrated += 1;
                            let indent = leading_ws_of_line(src, lt);
                            let open = rebuild_open_tag(&el, &["*ngIf"]);
                            let close = if el.self_closing {
                                String::new()
                            } else {
                                format!("</{}>", el.name)
                            };
                            let mut block = open;
                            if !el.self_closing {
                                let inner = process_range(src, el.content_range.clone(), stats);
                                block.push_str(&inner);
                                block.push_str(&close);
                            }
                            out.push_str(&format!("{indent}@if ({expr}) {{\n"));
                            out.push_str(&reindent(&block, &format!("{indent}  ")));
                            out.push('\n');
                            out.push_str(&indent);
                            out.push_str("}\n");
                            cursor = el.close_end.max(el.open_range.end);
                        }
                        Decision::MigrateFor(f) => {
                            stats.migrated += 1;
                            let track = match &f.track_by {
                                Some(tb) => format!("{tb}($index, {})", f.item),
                                None => f.item.to_string(),
                            };
                            let indent = leading_ws_of_line(src, lt);
                            let open = rebuild_open_tag(&el, &["*ngFor"]);
                            let close = if el.self_closing {
                                String::new()
                            } else {
                                format!("</{}>", el.name)
                            };
                            let mut block = open;
                            if !el.self_closing {
                                let inner = process_range(src, el.content_range.clone(), stats);
                                block.push_str(&inner);
                                block.push_str(&close);
                            }
                            out.push_str(&format!(
                                "{indent}@for ({} of {}; track {track}) {{\n",
                                f.item, f.iterable
                            ));
                            out.push_str(&reindent(&block, &format!("{indent}  ")));
                            out.push('\n');
                            out.push_str(&indent);
                            out.push_str("}\n");
                            cursor = el.close_end.max(el.open_range.end);
                        }
                        Decision::Skip(reason) => {
                            stats.skipped += 1;
                            stats
                                .warnings
                                .push(format!("skipped `{}` at byte {}: {reason}", el.name, lt));
                            // Recurse into content so nested directives still migrate.
                            let open_raw = &src[el.open_range.clone()];
                            let inner = process_range(src, el.content_range.clone(), stats);
                            let close_raw = if el.has_close {
                                &src[el.content_range.end..el.close_end]
                            } else {
                                ""
                            };
                            out.push_str(open_raw);
                            out.push_str(&inner);
                            out.push_str(close_raw);
                            cursor = el.close_end.max(el.open_range.end);
                        }
                        Decision::Keep => {
                            let open_raw = &src[el.open_range.clone()];
                            let inner = process_range(src, el.content_range.clone(), stats);
                            let close_raw = if el.has_close {
                                &src[el.content_range.end..el.close_end]
                            } else {
                                ""
                            };
                            out.push_str(open_raw);
                            out.push_str(&inner);
                            out.push_str(close_raw);
                            cursor = el.close_end.max(el.open_range.end);
                        }
                    }
                } else {
                    // Not a tag (e.g. `<` in text): emit and advance.
                    out.push_str(&src[lt..=lt]);
                    cursor = lt + 1;
                }
            }
            None => {
                out.push_str(&src[cursor..range.end]);
                cursor = range.end;
            }
        }
    }
    out
}

/// Apply the control-flow migration to all `.html` files under `root`.
pub fn apply_control_flow(root: &Path, dry: bool) -> Result<ControlFlowResult> {
    let mut result = ControlFlowResult::default();
    let re_ngif = Regex::new(r"\*ngIf|\*ngFor").unwrap();

    let mut files: Vec<PathBuf> = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            if e.file_type().is_dir() {
                return !matches!(
                    e.file_name().to_string_lossy().as_ref(),
                    "node_modules" | "dist" | ".git" | ".angular" | "coverage"
                );
            }
            true
        })
        .flatten()
    {
        if entry.file_type().is_file() && entry.file_name().to_string_lossy().ends_with(".html") {
            files.push(entry.into_path());
        }
    }

    for path in files {
        let raw = std::fs::read_to_string(&path)?;
        if !re_ngif.is_match(&raw) {
            continue;
        }
        let mut stats = Stats {
            migrated: 0,
            skipped: 0,
            warnings: Vec::new(),
        };
        let next = process_range(&raw, 0..raw.len(), &mut stats);
        result.migrated += stats.migrated;
        result.skipped += stats.skipped;
        result.warnings.extend(
            stats
                .warnings
                .into_iter()
                .map(|w| format!("{}: {w}", path.display())),
        );
        if next != raw {
            if !dry {
                std::fs::write(&path, next)?;
            }
            result.changed.push(path);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_simple_ngif() {
        let src = r#"<div class="x" *ngIf="show">
  <span>Hi</span>
</div>"#;
        let mut stats = Stats {
            migrated: 0,
            skipped: 0,
            warnings: Vec::new(),
        };
        let out = process_range(src, 0..src.len(), &mut stats);
        assert_eq!(stats.migrated, 1);
        assert!(out.contains("@if (show) {"));
        assert!(!out.contains("*ngIf"));
        assert!(out.contains("<span>Hi</span>"));
        assert!(out.contains("</div>"));
    }

    #[test]
    fn migrates_simple_ngfor() {
        let src = r#"<li *ngFor="let item of items">{{ item }}</li>"#;
        let mut stats = Stats {
            migrated: 0,
            skipped: 0,
            warnings: Vec::new(),
        };
        let out = process_range(src, 0..src.len(), &mut stats);
        assert!(out.contains("@for (item of items; track item) {"));
        assert!(!out.contains("*ngFor"));
    }

    #[test]
    fn migrates_trackby() {
        let src = r#"<li *ngFor="let item of items; trackBy: trackFn">{{ item }}</li>"#;
        let mut stats = Stats {
            migrated: 0,
            skipped: 0,
            warnings: Vec::new(),
        };
        let out = process_range(src, 0..src.len(), &mut stats);
        assert!(out.contains("@for (item of items; track trackFn($index, item)) {"));
    }

    #[test]
    fn skips_index_variable() {
        let src = r#"<li *ngFor="let item of items; let i = index">{{ i }}: {{ item }}</li>"#;
        let mut stats = Stats {
            migrated: 0,
            skipped: 0,
            warnings: Vec::new(),
        };
        let out = process_range(src, 0..src.len(), &mut stats);
        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.migrated, 0);
        assert!(out.contains("*ngFor"));
    }

    #[test]
    fn skips_ng_template_else() {
        let src = r#"<div *ngIf="cond; else other">A</div>"#;
        let mut stats = Stats {
            migrated: 0,
            skipped: 0,
            warnings: Vec::new(),
        };
        let out = process_range(src, 0..src.len(), &mut stats);
        assert_eq!(stats.skipped, 1);
        assert!(out.contains("*ngIf"));
    }

    #[test]
    fn migrates_nested() {
        let src = r#"<div *ngIf="a">
  <p *ngFor="let x of xs">{{ x }}</p>
</div>"#;
        let mut stats = Stats {
            migrated: 0,
            skipped: 0,
            warnings: Vec::new(),
        };
        let out = process_range(src, 0..src.len(), &mut stats);
        assert_eq!(stats.migrated, 2);
        assert!(out.contains("@if (a) {"));
        assert!(out.contains("@for (x of xs; track x) {"));
    }

    #[test]
    fn handles_self_closing() {
        let src = r#"<span *ngIf="ok" />"#;
        let mut stats = Stats {
            migrated: 0,
            skipped: 0,
            warnings: Vec::new(),
        };
        let out = process_range(src, 0..src.len(), &mut stats);
        assert!(out.contains("@if (ok) {"));
        assert!(out.contains("<span />"));
    }

    #[test]
    fn keeps_non_structural_elements() {
        let src = r#"<p>hello {{ name }}</p><hr>"#;
        let mut stats = Stats {
            migrated: 0,
            skipped: 0,
            warnings: Vec::new(),
        };
        let out = process_range(src, 0..src.len(), &mut stats);
        assert_eq!(out, src);
    }
}

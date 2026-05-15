use anyhow::Result;
use std::path::Path;

use crate::highlight::Span;
use crate::theme::Style;

/// Result of looking up a command name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandType {
    Builtin,
    Executable,
    Missing,
}

/// Result of looking up a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathType {
    File,
    Directory,
}

/// Environment for dynamic lookups.
pub struct LookupEnv {
    pub resolve_command: Box<dyn Fn(&str) -> CommandType + Send + Sync>,
    pub resolve_path: Box<dyn Fn(&str) -> Option<PathType> + Send + Sync>,
}

impl Default for LookupEnv {
    fn default() -> Self {
        Self {
            resolve_command: Box::new(real_resolve_command),
            resolve_path: Box::new(real_resolve_path(None)),
        }
    }
}

impl LookupEnv {
    /// Create a LookupEnv with a specific working directory for relative path resolution.
    pub fn with_pwd(pwd: &str) -> Self {
        let pwd = pwd.to_string();
        Self {
            resolve_command: Box::new(real_resolve_command),
            resolve_path: Box::new(real_resolve_path(Some(pwd))),
        }
    }
}

fn real_resolve_command(name: &str) -> CommandType {
    if is_builtin(name) {
        CommandType::Builtin
    } else if is_in_path(name) {
        CommandType::Executable
    } else {
        CommandType::Missing
    }
}

fn real_resolve_path(pwd: Option<String>) -> impl Fn(&str) -> Option<PathType> {
    move |path: &str| {
        let expanded = if path.starts_with('~') {
            if let Some(home) = std::env::var_os("HOME") {
                let home_str = home.to_string_lossy();
                if path.len() == 1 {
                    home_str.into_owned()
                } else if path.starts_with("~/") {
                    format!("{}{}", home_str, &path[1..])
                } else {
                    path.to_string()
                }
            } else {
                path.to_string()
            }
        } else if path.starts_with("./") || path.starts_with("../") {
            // Resolve relative paths using the provided pwd
            if let Some(ref pwd) = pwd {
                format!("{}/{}", pwd.trim_end_matches('/'), path)
            } else {
                path.to_string()
            }
        } else if path.contains('/') && pwd.is_some() {
            // Bare relative path like "foo/bar" — resolve against pwd
            format!("{}/{}", pwd.as_ref().unwrap().trim_end_matches('/'), path)
        } else {
            path.to_string()
        };
        let p = Path::new(&expanded);
        if p.is_dir() {
            Some(PathType::Directory)
        } else if p.is_file() {
            Some(PathType::File)
        } else {
            None
        }
    }
}

fn is_builtin(name: &str) -> bool {
    const BUILTINS: &[&str] = &[
        "alias", "autoload", "bg", "bindkey", "break", "builtin", "cd", "chdir", "command",
        "compadd", "comparguments", "compcall", "compctl", "compdescribe", "compfiles",
        "compgroups", "compquote", "comptags", "comptry", "compvalues", "continue", "dirs",
        "disable", "disown", "echo", "echotc", "echoti", "emulate", "enable", "eval", "exec",
        "exit", "export", "false", "fc", "fg", "float", "functions", "getln", "getopts",
        "hash", "history", "integer", "jobs", "kill", "let", "limit", "local", "log", "noglob",
        "popd", "print", "printf", "pushd", "pushln", "pwd", "read", "readonly", "rehash",
        "return", "sched", "set", "setopt", "shift", "source", "stat", "suspend", "test",
        "times", "trap", "true", "ttyctl", "type", "typeset", "ulimit", "umask", "unalias",
        "unfunction", "unhash", "unlimit", "unset", "unsetopt", "vared", "wait", "whence",
        "where", "which", "zcompile", "zformat", "zle", "zmodload", "zparseopts", "zregexparse",
        "zstat", "zstyle",
        // bash compat
        "bash", "sh", ".[", "[[", "]", "]]", "case", "do", "done", "elif", "else", "esac",
        "fi", "for", "function", "if", "in", "select", "then", "until", "while",
    ];
    BUILTINS.contains(&name)
}

fn is_in_path(name: &str) -> bool {
    if name.is_empty() || name.contains('/') {
        return false;
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            let full = Path::new(dir).join(name);
            if full.is_file() {
                return true;
            }
        }
    }
    false
}

/// Run dynamic highlighting on zsh source and return additional spans.
/// Accepts a pre-parsed tree to avoid double-parsing.
pub fn dynamic_highlight_zsh(
    tree: &tree_sitter::Tree,
    source: &str,
    lookups: &LookupEnv,
) -> Result<Vec<Span>> {
    let mut spans = Vec::new();

    let root = tree.root_node();
    walk_node(root, source, lookups, &mut spans);

    Ok(spans)
}

fn walk_node(node: tree_sitter::Node, source: &str, lookups: &LookupEnv, spans: &mut Vec<Span>) {
    match node.kind() {
        "command_name" => {
            let text = node_text(node, source);
            match (lookups.resolve_command)(&text) {
                CommandType::Missing => {
                    spans.push(Span {
                        start: node_start_char(node, source),
                        end: node_end_char(node, source),
                        style: Style::new().fg("#f7768e").to_ansi(),
                    });
                }
                _ => {}
            }
        }
        "word" | "string" => {
            let text = node_text(node, source);
            if looks_like_path(&text) {
                if let Some(path_type) = (lookups.resolve_path)(&text) {
                    let style = match path_type {
                        PathType::File => Style::new().underline(),
                        PathType::Directory => Style::new().underline().fg("#7aa2f7"),
                    };
                    spans.push(Span {
                        start: node_start_char(node, source),
                        end: node_end_char(node, source),
                        style: style.to_ansi(),
                    });
                }
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_node(child, source, lookups, spans);
    }
}

fn node_text(node: tree_sitter::Node, source: &str) -> String {
    source[node.start_byte()..node.end_byte()].to_string()
}

fn node_start_char(node: tree_sitter::Node, source: &str) -> usize {
    source[..node.start_byte()].chars().count()
}

fn node_end_char(node: tree_sitter::Node, source: &str) -> usize {
    source[..node.end_byte()].chars().count()
}

fn looks_like_path(text: &str) -> bool {
    if text.starts_with('/') || text.starts_with("./") || text.starts_with("../") {
        return true;
    }
    if text.starts_with('~') {
        return true;
    }
    if text.contains('/') {
        return true;
    }
    false
}

/// Blend static and dynamic spans. Dynamic spans override static styles where they overlap.
///
/// Base spans may overlap (e.g., a `string` span containing an `embedded` span containing a
/// `command` span). Following `region_highlight` last-wins semantics, the most specific (latest
/// in sorted order) active base span wins for each position.
pub fn blend_spans(base: Vec<Span>, mixins: Vec<Span>) -> Vec<Span> {
    if mixins.is_empty() {
        return base;
    }
    if base.is_empty() {
        return mixins;
    }

    let mut mixins = mixins;
    mixins.sort_unstable_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));

    let mut positions = Vec::new();
    for s in base.iter().chain(mixins.iter()) {
        positions.push(s.start);
        positions.push(s.end);
    }
    positions.sort_unstable();
    positions.dedup();

    let mut result: Vec<Span> = Vec::new();
    let mut bi = 0;
    let mut mi = 0;

    for w in positions.windows(2) {
        let (lo, hi) = (w[0], w[1]);

        // Advance past base spans that end before this window
        while bi < base.len() && base[bi].end <= lo {
            bi += 1;
        }
        // Advance past mixin spans that end before this window
        while mi < mixins.len() && mixins[mi].end <= lo {
            mi += 1;
        }

        // Find the most specific active base span (last one that covers this window).
        // Base spans are sorted by (start, -end), so later spans at the same start are
        // shorter/more specific. For overlapping spans, the last matching one wins
        // (region_highlight last-wins semantics).
        let active_base = base[bi..]
            .iter()
            .take_while(|s| s.start <= lo)
            .filter(|s| hi <= s.end)
            .last();

        let active_mixin = mixins.get(mi).filter(|s| s.start <= lo && hi <= s.end);

        let style = match (active_base, active_mixin) {
            (Some(b), Some(m)) => Some(mix_ansi(&b.style, &m.style)),
            (Some(b), None) => Some(b.style.clone()),
            (None, Some(m)) => Some(m.style.clone()),
            (None, None) => None,
        };

        if let Some(style) = style {
            if let Some(last) = result.last_mut() {
                if last.end == lo && last.style == style {
                    last.end = hi;
                    continue;
                }
            }
            result.push(Span {
                start: lo,
                end: hi,
                style,
            });
        }
    }

    result
}

/// Mix two ANSI style strings: mixin overrides base attributes.
fn mix_ansi(base: &str, mixin: &str) -> String {
    let mut fg = None;
    let mut bg = None;
    let mut bold = false;
    let mut italic = false;
    let mut underline = false;

    // Parse base
    for part in base.split(',') {
        if let Some(v) = part.strip_prefix("fg=") {
            fg = Some(v.to_string());
        } else if let Some(v) = part.strip_prefix("bg=") {
            bg = Some(v.to_string());
        } else if part == "bold" {
            bold = true;
        } else if part == "italic" {
            italic = true;
        } else if part == "underline" {
            underline = true;
        }
    }

    // Parse mixin (overrides base)
    for part in mixin.split(',') {
        if let Some(v) = part.strip_prefix("fg=") {
            fg = Some(v.to_string());
        } else if let Some(v) = part.strip_prefix("bg=") {
            bg = Some(v.to_string());
        } else if part == "bold" {
            bold = true;
        } else if part == "italic" {
            italic = true;
        } else if part == "underline" {
            underline = true;
        }
    }

    let mut parts = Vec::new();
    if let Some(fg) = fg {
        parts.push(format!("fg={fg}"));
    }
    if let Some(bg) = bg {
        parts.push(format!("bg={bg}"));
    }
    if bold {
        parts.push("bold".to_string());
    }
    if italic {
        parts.push("italic".to_string());
    }
    if underline {
        parts.push("underline".to_string());
    }
    parts.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_zsh(source: &str) -> tree_sitter::Tree {
        let language: tree_sitter::Language = tree_sitter_zsh::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).unwrap();
        parser.parse(source, None).unwrap()
    }

    #[test]
    fn blend_empty() {
        let base = vec![Span {
            start: 0,
            end: 5,
            style: "fg=#ff0000".to_string(),
        }];
        let mixins = vec![];
        let result = blend_spans(base.clone(), mixins);
        assert_eq!(result, base);
    }

    #[test]
    fn blend_mixin_overrides() {
        let base = vec![Span {
            start: 0,
            end: 10,
            style: "fg=#ff0000".to_string(),
        }];
        let mixins = vec![Span {
            start: 3,
            end: 7,
            style: "underline".to_string(),
        }];
        let result = blend_spans(base, mixins);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].start, 0);
        assert_eq!(result[0].end, 3);
        assert_eq!(result[0].style, "fg=#ff0000");
        assert_eq!(result[1].start, 3);
        assert_eq!(result[1].end, 7);
        assert_eq!(result[1].style, "fg=#ff0000,underline");
        assert_eq!(result[2].start, 7);
        assert_eq!(result[2].end, 10);
        assert_eq!(result[2].style, "fg=#ff0000");
    }

    #[test]
    fn mix_ansi_override_fg() {
        let base = "fg=#ff0000,bold";
        let mixin = "fg=#00ff00";
        assert_eq!(mix_ansi(base, mixin), "fg=#00ff00,bold");
    }

    #[test]
    fn dynamic_missing_command() {
        let source = "xyzunknown cmd";
        let lookups = LookupEnv {
            resolve_command: Box::new(|_| CommandType::Missing),
            resolve_path: Box::new(|_| None),
        };
        let tree = parse_zsh(source);
        let spans = dynamic_highlight_zsh(&tree, source, &lookups).unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start, 0);
        assert_eq!(spans[0].end, 10);
        assert!(spans[0].style.contains("#f7768e"));
    }

    #[test]
    fn dynamic_existing_path() {
        let source = "cat /etc/passwd";
        let lookups = LookupEnv {
            resolve_command: Box::new(|_| CommandType::Builtin),
            resolve_path: Box::new(|_| Some(PathType::File)),
        };
        let tree = parse_zsh(source);
        let spans = dynamic_highlight_zsh(&tree, source, &lookups).unwrap();
        assert!(
            spans.iter().any(|s| s.style == "underline"),
            "expected underline span for path"
        );
    }

    #[test]
    fn blend_overlapping_base_picks_most_specific() {
        // Simulates: string [5..28] containing embedded [6..27] containing command [8..11]
        // With a dynamic mixin (underline) on [16..26] for a file path.
        // blend_spans should pick the most specific (last) active base span at each position.
        let base = vec![
            Span { start: 0, end: 4, style: "fg=#7aa2f7".into() },           // echo
            Span { start: 5, end: 28, style: "fg=#e0af68".into() },          // string
            Span { start: 6, end: 27, style: "fg=#73daca".into() },          // embedded
            Span { start: 8, end: 11, style: "fg=#7aa2f7".into() },          // cat
            Span { start: 12, end: 14, style: "fg=#ff9e64".into() },         // -v
            Span { start: 15, end: 16, style: "fg=#7882bf".into() },         // <
        ];
        let mixins = vec![
            Span { start: 16, end: 26, style: "underline".into() },          // /etc/paths
        ];
        let result = blend_spans(base, mixins);

        // Check that the embedded color (teal) appears for the $() delimiters
        assert!(
            result.iter().any(|s| s.style.contains("fg=#73daca") && s.start == 6 && s.end == 8),
            "expected embedded teal for '$(' at [6..8], got: {:?}",
            result
        );
        assert!(
            result.iter().any(|s| s.style.contains("fg=#73daca") && s.start == 11 && s.end == 12),
            "expected embedded teal for space at [11..12], got: {:?}",
            result
        );
        // The file path should have embedded teal + underline
        assert!(
            result.iter().any(|s| s.style.contains("fg=#73daca") && s.style.contains("underline")
                && s.start == 16 && s.end == 26),
            "expected embedded teal+underline for path at [16..26], got: {:?}",
            result
        );
        // The string quotes should be string orange
        assert!(
            result.iter().any(|s| s.style == "fg=#e0af68" && s.start == 5 && s.end == 6),
            "expected string orange for opening quote at [5..6], got: {:?}",
            result
        );
        assert!(
            result.iter().any(|s| s.style == "fg=#e0af68" && s.start == 27 && s.end == 28),
            "expected string orange for closing quote at [27..28], got: {:?}",
            result
        );
        // The ) before the closing quote should be embedded teal
        assert!(
            result.iter().any(|s| s.style.contains("fg=#73daca") && s.start == 26 && s.end == 27),
            "expected embedded teal for ')' at [26..27], got: {:?}",
            result
        );
    }
}

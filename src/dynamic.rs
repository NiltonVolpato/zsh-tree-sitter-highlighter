use anyhow::Result;
use std::path::Path;

use crate::highlight::Span;

/// Result of looking up a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathType {
    File,
    Directory,
}

/// Environment for dynamic lookups.
pub struct LookupEnv {
    pub resolve_path: Box<dyn Fn(&str) -> Option<PathType> + Send + Sync>,
}

impl Default for LookupEnv {
    fn default() -> Self {
        Self {
            resolve_path: Box::new(real_resolve_path(None)),
        }
    }
}

impl LookupEnv {
    /// Create a LookupEnv with a specific working directory for relative path resolution.
    pub fn with_pwd(pwd: &str) -> Self {
        let pwd = pwd.to_string();
        Self {
            resolve_path: Box::new(real_resolve_path(Some(pwd))),
        }
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

/// Run dynamic path highlighting on zsh source and return additional spans.
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
        "word" | "string" => {
            let text = node_text(node, source);
            if looks_like_path(&text) {
                if let Some(path_type) = (lookups.resolve_path)(&text) {
                    let style = match path_type {
                        PathType::File => "path",
                        PathType::Directory => "path.directory",
                    };
                    spans.push(Span {
                        start: node_start_char(node, source),
                        end: node_end_char(node, source),
                        style: style.to_string(),
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
    fn dynamic_existing_path() {
        let source = "cat /etc/passwd";
        let lookups = LookupEnv {
            resolve_path: Box::new(|_| Some(PathType::File)),
        };
        let tree = parse_zsh(source);
        let spans = dynamic_highlight_zsh(&tree, source, &lookups).unwrap();
        assert!(
            spans.iter().any(|s| s.style == "path"),
            "expected path span for existing file"
        );
    }

    /// Regression test: bare relative paths (e.g. "foo/bar") must be resolved
    /// against the provided pwd. If the `path.contains('/') && pwd.is_some()`
    /// branch in `real_resolve_path` were removed, this test would fail.
    #[test]
    fn dynamic_existing_path_bare_relative() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("sub");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(subdir.join("file.txt"), "hello").unwrap();

        let pwd = dir.path().to_str().unwrap().to_string();
        let resolve_fn = real_resolve_path(Some(pwd));

        // "sub/file.txt" is a bare relative path (no ./ prefix, no leading /)
        let result = resolve_fn("sub/file.txt");
        assert_eq!(result, Some(PathType::File), "bare relative path should resolve against pwd");

        // "sub/" should resolve as a directory
        let result = resolve_fn("sub/");
        assert_eq!(result, Some(PathType::Directory), "bare relative dir path should resolve against pwd");

        // A non-existent bare relative path should return None
        let result = resolve_fn("sub/nope.txt");
        assert_eq!(result, None, "non-existent bare relative path should return None");
    }
}

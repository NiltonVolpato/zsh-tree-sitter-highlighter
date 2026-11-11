use std::collections::HashSet;

use anyhow::{Context, Result};
use tree_sitter::StreamingIterator;
use tree_sitter_highlight::HighlightConfiguration;

use crate::dynamic::PathCache;
use crate::incremental::IncrementalParser;

/// A highlighted span with character (not byte) offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub style: String,
}

impl Span {
    fn new(range: std::ops::Range<usize>, style: String) -> Self {
        Self {
            start: range.start,
            end: range.end,
            style,
        }
    }
}

/// A command name in the zsh source, with character offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub start: usize,
    pub end: usize,
    pub name: String,
}

/// Everything one pass over a zsh buffer produces.
#[derive(Debug, Default)]
pub struct ZshAnalysis {
    /// Static (query) and dynamic (path) spans, sorted by start, longest first.
    pub spans: Vec<Span>,
    /// Every `command_name` node, in source order.
    pub commands: Vec<Command>,
    /// Names of functions defined in the buffer itself.
    pub local_functions: HashSet<String>,
}

/// Supported languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageConfig {
    Zsh,
    Markdown,
}

/// Extra zsh patterns appended to the grammar's highlights query. Captures in the
/// `treeizsh.` namespace are not styles: they feed command validation and path
/// lookups, so the whole analysis is a single query pass over a single parse.
const ZSH_EXTRA_QUERY: &str = "
(variable_ref (simple_variable_name)) @variable
(ERROR) @error
(command_name) @treeizsh.command
(function_definition name: _ @treeizsh.function_def)
(word) @treeizsh.path
(string) @treeizsh.path
";

/// What a zsh query capture is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZshCapture {
    Style,
    Command,
    FunctionDef,
    PathCandidate,
}

impl ZshCapture {
    fn from_name(name: &str) -> Self {
        match name {
            "treeizsh.command" => Self::Command,
            "treeizsh.function_def" => Self::FunctionDef,
            "treeizsh.path" => Self::PathCandidate,
            _ => Self::Style,
        }
    }
}

/// Holds the pre-configured highlight configurations for each language, plus the
/// per-buffer state (last parse tree, path lookups) reused across keystrokes.
pub struct HighlightEngine {
    zsh_query: tree_sitter::Query,
    /// Role of each `zsh_query` capture, by capture index.
    zsh_captures: Vec<ZshCapture>,
    zsh_parser: IncrementalParser,
    md_block_config: HighlightConfiguration,
    md_inline_config: HighlightConfiguration,
    /// `None` disables dynamic (filesystem) highlighting.
    paths: Option<PathCache>,
}

impl HighlightEngine {
    pub fn new() -> Result<Self> {
        let zsh_lang: tree_sitter::Language = tree_sitter_zsh::LANGUAGE.into();
        let md_lang: tree_sitter::Language = tree_sitter_md::LANGUAGE.into();
        let md_inline_lang: tree_sitter::Language = tree_sitter_md::INLINE_LANGUAGE.into();

        let zsh_query_text = format!("{}\n{}", tree_sitter_zsh::HIGHLIGHT_QUERY, ZSH_EXTRA_QUERY);
        let zsh_query = tree_sitter::Query::new(&zsh_lang, &zsh_query_text)
            .context("failed to create zsh highlight query")?;
        let zsh_captures = zsh_query
            .capture_names()
            .iter()
            .map(|name| ZshCapture::from_name(name))
            .collect();

        let mut md_block_config = HighlightConfiguration::new(
            md_lang,
            "markdown",
            tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
            tree_sitter_md::INJECTION_QUERY_BLOCK,
            "",
        )
        .context("failed to create markdown block highlight configuration")?;

        let mut md_inline_config = HighlightConfiguration::new(
            md_inline_lang,
            "markdown_inline",
            tree_sitter_md::HIGHLIGHT_QUERY_INLINE,
            tree_sitter_md::INJECTION_QUERY_INLINE,
            "",
        )
        .context("failed to create markdown inline highlight configuration")?;

        // Enable all captures so we can return semantic names directly.
        // Use raw pointers to avoid borrow checker issues with capture_names() + configure().
        let md_block_names = unsafe {
            let query_ptr = &md_block_config.query as *const tree_sitter::Query;
            (*query_ptr).capture_names().to_vec()
        };
        md_block_config.configure(&md_block_names);
        let md_inline_names = unsafe {
            let query_ptr = &md_inline_config.query as *const tree_sitter::Query;
            (*query_ptr).capture_names().to_vec()
        };
        md_inline_config.configure(&md_inline_names);

        Ok(Self {
            zsh_query,
            zsh_captures,
            zsh_parser: IncrementalParser::new(&zsh_lang)?,
            md_block_config,
            md_inline_config,
            paths: Some(PathCache::default()),
        })
    }

    /// Create an engine without dynamic highlighting (for tests).
    #[cfg(test)]
    pub fn new_without_dynamic() -> Result<Self> {
        let mut engine = Self::new()?;
        engine.paths = None;
        Ok(engine)
    }

    /// Create an engine whose path lookups go through `paths` (for tests).
    #[cfg(test)]
    pub fn with_path_cache(paths: PathCache) -> Result<Self> {
        let mut engine = Self::new()?;
        engine.paths = Some(paths);
        Ok(engine)
    }

    /// Drop cached filesystem lookups (e.g. at the start of a new command line).
    pub fn clear_caches(&mut self) {
        if let Some(paths) = self.paths.as_mut() {
            paths.clear();
        }
    }

    pub fn highlight(&mut self, lang: LanguageConfig, source: &str) -> Result<Vec<Span>> {
        match lang {
            LanguageConfig::Zsh => Ok(self.analyze_zsh(source, None)?.spans),
            LanguageConfig::Markdown => self.highlight_markdown(source),
        }
    }

    /// Parse `source` (incrementally, against the previous call's tree) and run one
    /// query pass collecting highlight spans, command names, local function
    /// definitions and path highlights. Relative paths resolve against `pwd`.
    pub fn analyze_zsh(&mut self, source: &str, pwd: Option<&str>) -> Result<ZshAnalysis> {
        let tree = self.zsh_parser.parse(source)?;

        let byte_to_char = build_byte_to_char_table(source);
        let char_len = source.chars().count();
        let char_range = |node: tree_sitter::Node| {
            byte_to_char[node.start_byte()]..byte_to_char[node.end_byte()]
        };

        let mut static_spans = Vec::new();
        let mut dynamic_spans = Vec::new();
        let mut analysis = ZshAnalysis::default();

        let names = self.zsh_query.capture_names();
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut captures = cursor.captures(&self.zsh_query, tree.root_node(), source.as_bytes());
        while let Some((m, capture_idx)) = captures.next() {
            let capture = m.captures[*capture_idx];
            let index = capture.index as usize;
            let node = capture.node;
            let text = &source[node.start_byte()..node.end_byte()];
            match self.zsh_captures[index] {
                ZshCapture::Style => {
                    if let Some(span) = capture_to_span(names[index], node, &byte_to_char, char_len)
                    {
                        static_spans.push(span);
                    }
                }
                ZshCapture::Command => {
                    let range = char_range(node);
                    analysis.commands.push(Command {
                        start: range.start,
                        end: range.end,
                        name: text.to_string(),
                    });
                }
                ZshCapture::FunctionDef => {
                    analysis.local_functions.insert(text.to_string());
                }
                // Words in command position are validated as commands instead.
                ZshCapture::PathCandidate if !is_command_name(node) => {
                    if let Some(kind) = self.paths.as_mut().and_then(|p| p.lookup(text, pwd)) {
                        dynamic_spans.push(Span::new(char_range(node), kind.style().to_string()));
                    }
                }
                ZshCapture::PathCandidate => {}
            }
        }

        static_spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        // Dynamic spans go first so that, among equal ranges, static styles win.
        dynamic_spans.extend(merge_spans(static_spans));
        dynamic_spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        analysis.spans = dynamic_spans;
        Ok(analysis)
    }

    /// The style name of zsh capture `index`, or `None` for internal captures.
    fn zsh_style_name(&self, index: u32) -> Option<&str> {
        let index = index as usize;
        (self.zsh_captures[index] == ZshCapture::Style)
            .then(|| self.zsh_query.capture_names()[index])
    }

    fn highlight_markdown(&self, source: &str) -> Result<Vec<Span>> {
        // Markdown requires trailing newline.
        let owned_source;
        let source_to_parse = if !source.ends_with('\n') {
            owned_source = format!("{source}\n");
            owned_source.as_str()
        } else {
            source
        };

        let mut parser = tree_sitter_md::MarkdownParser::default();
        let md_tree = parser
            .parse(source_to_parse.as_bytes(), None)
            .context("markdown parsing failed")?;

        let byte_to_char = build_byte_to_char_table(source_to_parse);
        let char_len = source_to_parse.chars().count();
        let mut spans = Vec::new();
        let text = source_to_parse.as_bytes();

        // 1. Block-level captures (skip inline nodes and code_fence_content – handled separately).
        {
            let mut cursor = tree_sitter::QueryCursor::new();
            let mut captures = cursor.captures(
                &self.md_block_config.query,
                md_tree.block_tree().root_node(),
                text,
            );
            loop {
                captures.advance();
                if let Some((m, capture_idx)) = captures.get() {
                    let capture = m.captures[*capture_idx];
                    let node = capture.node;
                    if node.kind() == "code_fence_content" {
                        continue;
                    }
                    let capture_name = self.md_block_config.names()[capture.index as usize];
                    if let Some(span) = capture_to_span(capture_name, node, &byte_to_char, char_len)
                    {
                        spans.push(span);
                    }
                } else {
                    break;
                }
            }
        }

        // 2. Inline captures for every inline node.
        let mut inline_nodes = Vec::new();
        collect_nodes_by_kind(
            md_tree.block_tree().root_node(),
            "inline",
            &mut inline_nodes,
        );
        for inline_node in inline_nodes {
            if let Some(inline_tree) = md_tree.inline_tree(&inline_node) {
                let mut cursor = tree_sitter::QueryCursor::new();
                let mut captures =
                    cursor.captures(&self.md_inline_config.query, inline_tree.root_node(), text);
                loop {
                    captures.advance();
                    if let Some((m, capture_idx)) = captures.get() {
                        let capture = m.captures[*capture_idx];
                        let capture_name = self.md_inline_config.names()[capture.index as usize];
                        if let Some(span) =
                            capture_to_span(capture_name, capture.node, &byte_to_char, char_len)
                        {
                            spans.push(span);
                        }
                    } else {
                        break;
                    }
                }
            }
        }

        // 3. Zsh injection inside fenced code blocks.
        let mut code_blocks = Vec::new();
        collect_nodes_by_kind(
            md_tree.block_tree().root_node(),
            "fenced_code_block",
            &mut code_blocks,
        );
        for block in code_blocks {
            let mut language = None;
            let mut code_range = None;
            let mut cursor = block.walk();
            for child in block.children(&mut cursor) {
                match child.kind() {
                    "info_string" => {
                        let mut info_cursor = child.walk();
                        for info_child in child.children(&mut info_cursor) {
                            if info_child.kind() == "language" {
                                language = Some(
                                    &source_to_parse
                                        [info_child.start_byte()..info_child.end_byte()],
                                );
                            }
                        }
                    }
                    "code_fence_content" => {
                        code_range = Some((child.start_byte(), child.end_byte()));
                    }
                    _ => {}
                }
            }

            if matches!(
                language,
                Some("zsh") | Some("bash") | Some("sh") | Some("shell")
            ) {
                if let Some((code_start, code_end)) = code_range {
                    let code = &source_to_parse[code_start..code_end];
                    let zsh_lang: tree_sitter::Language = tree_sitter_zsh::LANGUAGE.into();
                    let mut zsh_parser = tree_sitter::Parser::new();
                    zsh_parser.set_language(&zsh_lang)?;
                    let zsh_tree = zsh_parser.parse(code, None);

                    if let Some(zsh_tree) = zsh_tree {
                        let code_byte_to_char = build_byte_to_char_table(code);
                        let code_char_len = code.chars().count();
                        let mut qcursor = tree_sitter::QueryCursor::new();
                        let mut captures = qcursor.captures(
                            &self.zsh_query,
                            zsh_tree.root_node(),
                            code.as_bytes(),
                        );
                        loop {
                            captures.advance();
                            if let Some((m, capture_idx)) = captures.get() {
                                let capture = m.captures[*capture_idx];
                                let node = capture.node;
                                let Some(capture_name) = self.zsh_style_name(capture.index) else {
                                    continue;
                                };
                                let start = node.start_byte();
                                let end = node.end_byte();
                                let source_len = code_byte_to_char.len().saturating_sub(1);
                                let start = start.min(source_len);
                                let end = end.min(source_len);
                                let local_char_start = code_byte_to_char[start].min(code_char_len);
                                let local_char_end = code_byte_to_char[end].min(code_char_len);
                                if local_char_start >= local_char_end {
                                    continue;
                                }
                                let doc_char_start =
                                    byte_to_char[(code_start + start).min(byte_to_char.len() - 1)];
                                let doc_char_end =
                                    byte_to_char[(code_start + end).min(byte_to_char.len() - 1)];
                                if capture_name != "none" {
                                    spans.push(Span::new(
                                        doc_char_start..doc_char_end,
                                        capture_name.to_string(),
                                    ));
                                }
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Sort by start position; for equal starts, longer spans come first so that
        // region_highlight's "last wins" gives precedence to shorter (more specific) spans.
        spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        Ok(merge_spans(spans))
    }

    /// Returns every capture name referenced by the loaded queries.
    #[cfg(test)]
    pub fn all_capture_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for (index, name) in self.zsh_query.capture_names().iter().enumerate() {
            if self.zsh_captures[index] == ZshCapture::Style && !names.contains(&name.to_string()) {
                names.push(name.to_string());
            }
        }
        for config in [&self.md_block_config, &self.md_inline_config] {
            for name in config.names() {
                if !names.contains(&name.to_string()) {
                    names.push(name.to_string());
                }
            }
        }
        names.sort();
        names
    }
}

fn capture_to_span(
    capture_name: &str,
    node: tree_sitter::Node,
    byte_to_char: &[usize],
    char_len: usize,
) -> Option<Span> {
    // Skip meta captures used for injection/language detection.
    if capture_name == "none"
        || capture_name == "injection.content"
        || capture_name == "injection.language"
    {
        return None;
    }
    let source_len = byte_to_char.len().saturating_sub(1);
    let start = node.start_byte().min(source_len);
    let end = node.end_byte().min(source_len);
    let char_start = byte_to_char[start].min(char_len);
    let char_end = byte_to_char[end].min(char_len);
    if char_start >= char_end {
        return None;
    }
    Some(Span::new(char_start..char_end, capture_name.to_string()))
}

/// Whether `node` is (part of) the command name of a simple command.
fn is_command_name(node: tree_sitter::Node) -> bool {
    node.parent().is_some_and(|p| p.kind() == "command_name")
}

/// Build a lookup table mapping byte offset -> character index.
fn build_byte_to_char_table(source: &str) -> Vec<usize> {
    let mut table = vec![0; source.len() + 1];
    let mut char_idx = 0;
    for (byte_idx, ch) in source.char_indices() {
        table[byte_idx] = char_idx;
        for i in 1..ch.len_utf8() {
            table[byte_idx + i] = char_idx;
        }
        char_idx += 1;
    }
    table[source.len()] = char_idx;
    table
}

/// Merge adjacent spans with identical styles.
fn merge_spans(spans: Vec<Span>) -> Vec<Span> {
    if spans.is_empty() {
        return spans;
    }
    let mut merged: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        if let Some(last) = merged.last_mut() {
            if last.end == span.start && last.style == span.style {
                last.end = span.end;
                continue;
            }
        }
        merged.push(span);
    }
    merged
}

fn collect_nodes_by_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
    result: &mut Vec<tree_sitter::Node<'a>>,
) {
    if node.kind() == kind {
        result.push(node);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_nodes_by_kind(child, kind, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use googletest::prelude::*;

    #[test]
    fn byte_to_char_with_ascii() {
        let t = build_byte_to_char_table("hello");
        assert_eq!(t, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn byte_to_char_with_multi_byte() {
        let t = build_byte_to_char_table("aéb");
        assert_eq!(t, vec![0, 1, 1, 2, 3]);
    }

    #[test]
    fn byte_to_char_with_emoji() {
        let t = build_byte_to_char_table("🐑");
        assert_eq!(t.len(), 5); // 4 bytes + 1
        assert_eq!(t[0], 0);
        assert_eq!(t[1], 0);
        assert_eq!(t[2], 0);
        assert_eq!(t[3], 0);
        assert_eq!(t[4], 1);
    }

    fn engine() -> HighlightEngine {
        HighlightEngine::new_without_dynamic().unwrap()
    }

    fn spans_to_segments(spans: &[Span], source: &str) -> Vec<(String, String)> {
        spans
            .iter()
            .map(|s| {
                let text: String = source.chars().skip(s.start).take(s.end - s.start).collect();
                (text.to_string(), s.style.to_string())
            })
            .collect()
    }

    fn assert_highlights(source: &str, lang: LanguageConfig, expected: &[(&str, &str)]) {
        let mut e = engine();
        let spans = e.highlight(lang, source).unwrap();
        let expected_string: Vec<(String, String)> = expected
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        assert_that!(spans_to_segments(&spans, source), eq(&expected_string));
    }

    fn assert_zsh_highlights(source: &str, expected: &[(&str, &str)]) {
        assert_highlights(source, LanguageConfig::Zsh, expected);
    }

    fn assert_md_highlights(source: &str, expected: &[(&str, &str)]) {
        assert_highlights(source, LanguageConfig::Markdown, expected);
    }

    #[test]
    fn snapshot_zsh_echo_hello() {
        assert_zsh_highlights("echo hello", &[("echo", "function")]);
    }

    #[test]
    fn snapshot_zsh_combined_static_dynamic() {
        let mut e = HighlightEngine::new().unwrap();
        let spans = e.highlight(LanguageConfig::Zsh, "echo hello").unwrap();
        assert_that!(
            spans_to_segments(&spans, "echo hello"),
            eq(&vec![("echo".into(), "function".into())])
        );
    }

    #[test]
    fn snapshot_zsh_comment() {
        assert_zsh_highlights("# foo bar", &[("# foo bar", "comment")]);
    }

    #[test]
    fn snapshot_zsh_quoted_string() {
        assert_zsh_highlights(
            r#"echo "hello world""#,
            &[("echo", "function"), ("\"hello world\"", "string")],
        );
    }

    #[test]
    fn snapshot_zsh_variable() {
        assert_zsh_highlights("echo $HOME", &[("echo", "function"), ("$HOME", "variable")]);
    }

    #[test]
    fn snapshot_zsh_variable_in_quotes() {
        assert_zsh_highlights(
            r#"echo "foo $BAR""#,
            &[
                ("echo", "function"),
                ("\"foo $BAR\"", "string"),
                ("$BAR", "variable"),
            ],
        );
    }

    #[test]
    fn snapshot_zsh_subshell() {
        assert_zsh_highlights(
            "echo $(date)",
            &[
                ("echo", "function"),
                ("$(date)", "embedded"),
                ("date", "function"),
            ],
        );
    }

    #[test]
    fn snapshot_zsh_pipe_redir() {
        assert_zsh_highlights(
            "cat file | grep foo > out.txt",
            &[
                ("cat", "function"),
                ("|", "operator"),
                ("grep", "function"),
                (">", "operator"),
            ],
        );
    }

    #[test]
    fn snapshot_md_heading() {
        assert_md_highlights(
            "# Hello\n",
            &[("#", "punctuation.special"), ("Hello", "text.title")],
        );
    }

    #[test]
    fn snapshot_md_bold() {
        assert_md_highlights(
            "**hello**\n",
            &[
                ("**hello**", "text.strong"),
                ("**", "punctuation.delimiter"),
                ("**", "punctuation.delimiter"),
            ],
        );
    }

    #[test]
    fn snapshot_md_italic() {
        assert_md_highlights(
            "*hello*\n",
            &[
                ("*hello*", "text.emphasis"),
                ("*", "punctuation.delimiter"),
                ("*", "punctuation.delimiter"),
            ],
        );
    }

    #[test]
    fn snapshot_md_inline_code() {
        assert_md_highlights(
            "use `cmd` here\n",
            &[
                ("`cmd`", "text.literal"),
                ("`", "punctuation.delimiter"),
                ("`", "punctuation.delimiter"),
            ],
        );
    }

    #[test]
    fn snapshot_md_code_fence_zsh() {
        assert_md_highlights(
            "```zsh\necho hi\n```\n",
            &[
                ("```zsh\necho hi\n```\n", "text.literal"),
                ("```", "punctuation.delimiter"),
                ("echo", "function"),
                ("```", "punctuation.delimiter"),
            ],
        );
    }

    #[test]
    fn snapshot_md_link() {
        assert_md_highlights(
            "[link](https://example.com)\n",
            &[
                ("[", "punctuation.delimiter"),
                ("link", "text.reference"),
                ("](", "punctuation.delimiter"),
                ("https://example.com", "text.uri"),
                (")", "punctuation.delimiter"),
            ],
        );
    }

    #[test]
    fn snapshot_md_unordered_list() {
        assert_md_highlights(
            "- foo\n- bar\n",
            &[("- ", "punctuation.special"), ("- ", "punctuation.special")],
        );
    }

    #[test]
    fn snapshot_md_task_list() {
        assert_md_highlights(
            "- [x] done\n- [ ] todo\n",
            &[("- ", "punctuation.special"), ("- ", "punctuation.special")],
        );
    }
    #[test]
    fn highlight_md_code_block_with_zsh_injection() {
        let mut e = engine();
        let source = "```zsh\necho hi\n```";
        let spans = e.highlight(LanguageConfig::Markdown, source).unwrap();
        // Should contain zsh-style highlighting inside the code block
        let segments = spans_to_segments(&spans, source);
        assert!(
            segments
                .iter()
                .any(|(text, style)| text == "echo" && style == "function"),
            "expected zsh-style span for 'echo', got: {:?}",
            segments
        );
    }

    #[test]
    fn all_capture_names_have_theme_entries() {
        let e = engine();
        let names = e.all_capture_names();
        // With semantic names, every capture should have a non-empty name.
        assert!(
            names.iter().all(|n| !n.is_empty()),
            "empty capture names found: {:?}",
            names
        );
        assert!(
            names.iter().all(|n| !n.starts_with("treeizsh.")),
            "internal captures leaked: {:?}",
            names
        );
    }

    #[test]
    fn analyze_zsh_collects_commands_and_local_functions() {
        let mut e = engine();
        let a = e.analyze_zsh("é() { ls; }; é | grep x", None).unwrap();
        let commands: Vec<_> = a
            .commands
            .iter()
            .map(|c| (c.start, c.end, c.name.as_str()))
            .collect();
        // Offsets are in chars: "é" is one char even though it is two bytes.
        assert_eq!(
            commands,
            vec![(6, 8, "ls"), (13, 14, "é"), (17, 21, "grep")]
        );
        assert_eq!(a.local_functions, HashSet::from(["é".to_string()]));
        assert!(a.spans.iter().all(|s| !s.style.starts_with("treeizsh.")));
    }

    #[test]
    fn analyze_zsh_highlights_paths_and_memoizes_lookups() {
        use crate::dynamic::PathType;
        use std::sync::{Arc, Mutex};

        let looked_up = Arc::new(Mutex::new(Vec::new()));
        let log = looked_up.clone();
        let paths = PathCache::new(Box::new(move |path, _| {
            log.lock().unwrap().push(path.to_string());
            match path {
                "/etc" => Some(PathType::Directory),
                "Cargo.toml" | "ls" => Some(PathType::File),
                _ => None,
            }
        }));
        let mut e = HighlightEngine::with_path_cache(paths).unwrap();

        // Simulate typing; each prefix is a keystroke.
        let line = "ls /etc Cargo.toml nope";
        let mut last = Vec::new();
        for end in 1..=line.len() {
            last = e.analyze_zsh(&line[..end], Some("/")).unwrap().spans;
        }
        assert_eq!(
            spans_to_segments(&last, line),
            vec![
                // A file named `ls` exists, but in command position it's a command.
                ("ls".into(), "function".into()),
                ("/etc".into(), "path.directory".into()),
                ("Cargo.toml".into(), "path".into()),
            ]
        );
        // Each distinct word is looked up once, not once per keystroke, and the
        // command word never is.
        let mut seen = looked_up.lock().unwrap().clone();
        assert!(!seen.iter().any(|w| w == "l" || w == "ls"), "{seen:?}");
        let total = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), total, "repeated lookups");

        e.clear_caches();
        e.analyze_zsh(line, Some("/")).unwrap();
        assert_eq!(looked_up.lock().unwrap().len(), total + 3);
    }

    #[test]
    fn incremental_analysis_matches_fresh_engine() {
        let target = "for f in ./src/*.rs; do echo \"$f é\" | sed 's/a/b/'; done # end";
        let mut incremental = engine();
        let mut typed = String::new();
        for ch in target.chars() {
            typed.push(ch);
            let got = incremental.analyze_zsh(&typed, None).unwrap();
            let want = engine().analyze_zsh(&typed, None).unwrap();
            assert_eq!(got.spans, want.spans, "spans after typing {typed:?}");
            assert_eq!(
                got.commands, want.commands,
                "commands after typing {typed:?}"
            );
        }
    }
}

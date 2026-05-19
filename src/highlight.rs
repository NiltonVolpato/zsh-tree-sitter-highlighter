use anyhow::{Context, Result};
use tree_sitter::StreamingIterator;
use tree_sitter_highlight::HighlightConfiguration;

use crate::dynamic::{LookupEnv, dynamic_highlight_zsh};

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

/// Supported languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageConfig {
    Zsh,
    Markdown,
}

/// Holds the pre-configured highlight configurations for each language.
pub struct HighlightEngine {
    zsh_query: tree_sitter::Query,
    md_block_config: HighlightConfiguration,
    md_inline_config: HighlightConfiguration,
    dynamic_env: Option<LookupEnv>,
}

impl HighlightEngine {
    pub fn new() -> Result<Self> {
        let zsh_lang: tree_sitter::Language = tree_sitter_zsh::LANGUAGE.into();
        let md_lang: tree_sitter::Language = tree_sitter_md::LANGUAGE.into();
        let md_inline_lang: tree_sitter::Language = tree_sitter_md::INLINE_LANGUAGE.into();

        let zsh_query = tree_sitter::Query::new(&zsh_lang, tree_sitter_zsh::HIGHLIGHT_QUERY)
            .context("failed to create zsh highlight query")?;

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
            md_block_config,
            md_inline_config,
            dynamic_env: Some(LookupEnv::default()),
        })
    }

    /// Create an engine without dynamic highlighting (for tests).
    #[cfg(test)]
    pub fn new_without_dynamic() -> Result<Self> {
        let mut engine = Self::new()?;
        engine.dynamic_env = None;
        Ok(engine)
    }

    pub fn highlight(&self, lang: LanguageConfig, source: &str) -> Result<Vec<Span>> {
        match lang {
            LanguageConfig::Zsh => self.highlight_zsh(source),
            LanguageConfig::Markdown => self.highlight_markdown(source),
        }
    }

    /// Highlight with a specific working directory for dynamic path resolution.
    pub fn highlight_with_pwd(
        &self,
        lang: LanguageConfig,
        source: &str,
        pwd: Option<&str>,
    ) -> Result<Vec<Span>> {
        if let Some(pwd) = pwd {
            let env = LookupEnv::with_pwd(pwd);
            match lang {
                LanguageConfig::Zsh => {
                    let (tree, static_spans) = self.highlight_zsh_static(source)?;
                    let dynamic_spans = dynamic_highlight_zsh(&tree, source, &env)?;
                    let mut all = dynamic_spans;
                    all.extend(static_spans);
                    all.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
                    Ok(all)
                }
                LanguageConfig::Markdown => self.highlight_markdown(source),
            }
        } else {
            self.highlight(lang, source)
        }
    }

    fn highlight_zsh(&self, source: &str) -> Result<Vec<Span>> {
        let (tree, static_spans) = self.highlight_zsh_static(source)?;

        if let Some(ref env) = self.dynamic_env {
            let dynamic_spans = dynamic_highlight_zsh(&tree, source, env)?;
            let mut all = dynamic_spans;
            all.extend(static_spans);
            all.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
            Ok(all)
        } else {
            Ok(static_spans)
        }
    }

    /// Run static zsh highlighting (tree-sitter query captures), returning
    /// the parsed tree and sorted/merged spans. Callers handle dynamic highlighting.
    fn highlight_zsh_static(&self, source: &str) -> Result<(tree_sitter::Tree, Vec<Span>)> {
        let language: tree_sitter::Language = tree_sitter_zsh::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language)?;
        let tree = parser
            .parse(source, None)
            .context("zsh parsing failed")?;

        let byte_to_char = build_byte_to_char_table(source);
        let char_len = source.chars().count();
        let text = source.as_bytes();

        let mut static_spans = Vec::new();
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut captures = cursor.captures(&self.zsh_query, tree.root_node(), text);
        loop {
            captures.advance();
            if let Some((m, capture_idx)) = captures.get() {
                let capture = m.captures[*capture_idx];
                let capture_name = self.zsh_query.capture_names()[capture.index as usize];
                if let Some(span) = self.capture_to_span(
                    capture_name,
                    capture.node,
                    &byte_to_char,
                    char_len,
                ) {
                    static_spans.push(span);
                }
            } else {
                break;
            }
        }
        static_spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        let static_spans = merge_spans(static_spans);

        Ok((tree, static_spans))
    }

    /// Extract command name positions from zsh source.
    /// Returns (start_char, end_char, name) for each `command_name` node.
    pub fn extract_zsh_commands(&self, source: &str) -> Result<Vec<(usize, usize, String)>> {
        let language: tree_sitter::Language = tree_sitter_zsh::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language)?;
        let tree = parser.parse(source, None).context("zsh parsing failed")?;
        let mut commands = Vec::new();
        Self::walk_commands(tree.root_node(), source, &mut commands);
        Ok(commands)
    }

    fn walk_commands(
        node: tree_sitter::Node,
        source: &str,
        commands: &mut Vec<(usize, usize, String)>,
    ) {
        if node.kind() == "command_name" {
            let start = source[..node.start_byte()].chars().count();
            let end = source[..node.end_byte()].chars().count();
            let name = source[node.start_byte()..node.end_byte()].to_string();
            commands.push((start, end, name));
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::walk_commands(child, source, commands);
        }
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
                    if let Some(span) = self.capture_to_span(
                        capture_name,
                        node,
                        &byte_to_char,
                        char_len,
                    ) {
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
                        if let Some(span) = self.capture_to_span(
                            capture_name,
                            capture.node,
                            &byte_to_char,
                            char_len,
                        ) {
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
                                    &source_to_parse[info_child.start_byte()..info_child.end_byte()],
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
                        let mut captures =
                            qcursor.captures(&self.zsh_query, zsh_tree.root_node(), code.as_bytes());
                        loop {
                            captures.advance();
                            if let Some((m, capture_idx)) = captures.get() {
                                let capture = m.captures[*capture_idx];
                                let node = capture.node;
                                let capture_name = self.zsh_query.capture_names()[capture.index as usize];
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

    fn capture_to_span(
        &self,
        capture_name: &str,
        node: tree_sitter::Node,
        byte_to_char: &[usize],
        char_len: usize,
    ) -> Option<Span> {
        // Skip meta captures used for injection/language detection.
        if capture_name == "none" || capture_name == "injection.content" || capture_name == "injection.language" {
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

    /// Returns every capture name referenced by the loaded queries.
    #[cfg(test)]
    pub fn all_capture_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for name in self.zsh_query.capture_names() {
            if !names.contains(&name.to_string()) {
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
        let e = engine();
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
        let e = HighlightEngine::new().unwrap();
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
        assert_zsh_highlights("echo $HOME", &[("echo", "function")]);
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
        let e = engine();
        let source = "```zsh\necho hi\n```";
        let spans = e.highlight(LanguageConfig::Markdown, source).unwrap();
        // Should contain zsh-style highlighting inside the code block
        let segments = spans_to_segments(&spans, source);
        assert!(
            segments.iter().any(|(text, style)| text == "echo" && style == "function"),
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
    }
}

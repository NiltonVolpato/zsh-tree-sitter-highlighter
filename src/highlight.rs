use anyhow::{Context, Result};
use tree_sitter::StreamingIterator;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::dynamic::{LookupEnv, blend_spans, dynamic_highlight_zsh};
use crate::theme::Theme;

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
    zsh_config: HighlightConfiguration,
    md_block_config: HighlightConfiguration,
    md_inline_config: HighlightConfiguration,
    md_block_capture_map: Vec<Option<usize>>,
    md_inline_capture_map: Vec<Option<usize>>,
    theme: Theme,
    dynamic_env: Option<LookupEnv>,
}

impl HighlightEngine {
    pub fn new(theme: Theme) -> Result<Self> {
        let zsh_lang: tree_sitter::Language = tree_sitter_zsh::LANGUAGE.into();
        let md_lang: tree_sitter::Language = tree_sitter_md::LANGUAGE.into();
        let md_inline_lang: tree_sitter::Language = tree_sitter_md::INLINE_LANGUAGE.into();

        let mut zsh_config =
            HighlightConfiguration::new(zsh_lang, "zsh", tree_sitter_zsh::HIGHLIGHT_QUERY, "", "")
                .context("failed to create zsh highlight configuration")?;

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

        let names: Vec<&str> = theme.names().iter().map(|s| s.as_str()).collect();
        zsh_config.configure(&names);
        md_block_config.configure(&names);
        md_inline_config.configure(&names);

        let md_block_capture_map = build_capture_map(md_block_config.names(), theme.names());
        let md_inline_capture_map = build_capture_map(md_inline_config.names(), theme.names());

        Ok(Self {
            zsh_config,
            md_block_config,
            md_inline_config,
            md_block_capture_map,
            md_inline_capture_map,
            theme,
            dynamic_env: Some(LookupEnv::default()),
        })
    }

    /// Create an engine without dynamic highlighting (for tests).
    #[cfg(test)]
    pub fn new_without_dynamic(theme: Theme) -> Result<Self> {
        let mut engine = Self::new(theme)?;
        engine.dynamic_env = None;
        Ok(engine)
    }

    pub fn highlight(&self, lang: LanguageConfig, source: &str) -> Result<Vec<Span>> {
        match lang {
            LanguageConfig::Zsh => self.highlight_zsh(source),
            LanguageConfig::Markdown => self.highlight_markdown(source),
        }
    }

    fn highlight_zsh(&self, source: &str) -> Result<Vec<Span>> {
        let mut highlighter = Highlighter::new();
        let events = highlighter
            .highlight(&self.zsh_config, source.as_bytes(), None, |_| None)
            .context("highlighting failed")?;
        let static_spans = self.events_to_spans(source, events)?;

        if let Some(ref env) = self.dynamic_env {
            let dynamic_spans = dynamic_highlight_zsh(source, env)?;
            Ok(blend_spans(static_spans, dynamic_spans))
        } else {
            Ok(static_spans)
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
                    if let Some(span) = self.capture_to_span(
                        capture.index as usize,
                        node,
                        &byte_to_char,
                        char_len,
                        &self.md_block_capture_map,
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
                        if let Some(span) = self.capture_to_span(
                            capture.index as usize,
                            capture.node,
                            &byte_to_char,
                            char_len,
                            &self.md_inline_capture_map,
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
                    "language" => {
                        language = Some(&source_to_parse[child.start_byte()..child.end_byte()])
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
                    let mut highlighter = Highlighter::new();
                    let events = highlighter
                        .highlight(&self.zsh_config, code.as_bytes(), None, |_| None)
                        .context("zsh injection highlight failed")?;
                    // Convert injection events, offsetting back to document coords.
                    let mut stack: Vec<usize> = Vec::new();
                    for event in events {
                        let event = event.context("highlight event error")?;
                        match event {
                            HighlightEvent::HighlightStart(h) => stack.push(h.0),
                            HighlightEvent::HighlightEnd => {
                                stack.pop();
                            }
                            HighlightEvent::Source { start, end } => {
                                if start == end {
                                    continue;
                                }
                                let doc_byte_start = code_start + start;
                                let doc_byte_end = code_start + end;
                                let char_start =
                                    byte_to_char[doc_byte_start.min(byte_to_char.len() - 1)];
                                let char_end =
                                    byte_to_char[doc_byte_end.min(byte_to_char.len() - 1)];
                                if char_start == char_end {
                                    continue;
                                }
                                if let Some(&idx) = stack.last() {
                                    if let Some(style) = self.theme.style_by_index(idx) {
                                        let ansi = style.to_ansi();
                                        if !ansi.is_empty() {
                                            spans.push(Span::new(char_start..char_end, ansi));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(merge_spans(spans))
    }

    fn capture_to_span(
        &self,
        capture_index: usize,
        node: tree_sitter::Node,
        byte_to_char: &[usize],
        char_len: usize,
        capture_map: &[Option<usize>],
    ) -> Option<Span> {
        let source_len = byte_to_char.len().saturating_sub(1);
        let start = node.start_byte().min(source_len);
        let end = node.end_byte().min(source_len);
        let char_start = byte_to_char[start].min(char_len);
        let char_end = byte_to_char[end].min(char_len);
        if char_start >= char_end {
            return None;
        }
        let &theme_idx = capture_map.get(capture_index)?;
        let theme_idx = theme_idx?;
        let style = self.theme.style_by_index(theme_idx)?;
        let ansi = style.to_ansi();
        if ansi.is_empty() {
            return None;
        }
        Some(Span::new(char_start..char_end, ansi))
    }

    fn events_to_spans<'a>(
        &self,
        source: &str,
        events: impl Iterator<Item = Result<HighlightEvent, tree_sitter_highlight::Error>>,
    ) -> Result<Vec<Span>> {
        let byte_to_char = build_byte_to_char_table(source);
        let mut spans = Vec::new();
        let mut stack: Vec<usize> = Vec::new();

        for event in events {
            let event = event.context("highlight event error")?;
            match event {
                HighlightEvent::HighlightStart(h) => stack.push(h.0),
                HighlightEvent::HighlightEnd => {
                    stack.pop();
                }
                HighlightEvent::Source { start, end } => {
                    if start == end {
                        continue;
                    }
                    let char_start = byte_to_char[start];
                    let char_end = byte_to_char[end.min(byte_to_char.len() - 1)];
                    if char_start == char_end {
                        continue;
                    }
                    if let Some(&idx) = stack.last() {
                        if let Some(style) = self.theme.style_by_index(idx) {
                            let ansi = style.to_ansi();
                            if !ansi.is_empty() {
                                spans.push(Span::new(char_start..char_end, ansi));
                            }
                        }
                    }
                }
            }
        }
        Ok(merge_spans(spans))
    }

    /// Returns every capture name referenced by the loaded queries.
    #[cfg(test)]
    pub fn all_capture_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for config in [
            &self.zsh_config,
            &self.md_block_config,
            &self.md_inline_config,
        ] {
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

fn build_capture_map(query_names: &[&str], theme_names: &[String]) -> Vec<Option<usize>> {
    query_names
        .iter()
        .map(|capture_name| {
            let capture_parts: Vec<_> = capture_name.split('.').collect();
            let mut best_index = None;
            let mut best_match_len = 0;
            for (i, theme_name) in theme_names.iter().enumerate() {
                let theme_parts: Vec<_> = theme_name.split('.').collect();
                let mut len = 0;
                let mut matches = true;
                for part in &theme_parts {
                    len += 1;
                    if !capture_parts.contains(part) {
                        matches = false;
                        break;
                    }
                }
                if matches && len > best_match_len {
                    best_index = Some(i);
                    best_match_len = len;
                }
            }
            best_index
        })
        .collect()
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
    use crate::theme::tokyonight_dark;
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
        HighlightEngine::new_without_dynamic(tokyonight_dark()).unwrap()
    }

    fn fmt_spans(spans: &[Span], source: &str) -> String {
        let chars: Vec<char> = source.chars().collect();
        let mut lines = Vec::new();
        for s in spans {
            let text: String = chars[s.start..s.end.min(chars.len())].iter().collect();
            lines.push(format!("{}..{} {} = {:?}", s.start, s.end, s.style, text));
        }
        lines.join("\n") + "\n"
    }

    #[derive(Debug, PartialEq)]
    struct Segment {
        text: String,
        style: String,
    }

    impl Segment {
        fn new(text: &str, style: &str) -> Self {
            Self {
                text: text.to_string(),
                style: style.to_string(),
            }
        }
    }

    fn spans_to_segments(spans: &[Span], source: &str) -> Vec<Segment> {
        let chars: Vec<char> = source.chars().collect();
        spans
            .iter()
            .map(|s| {
                let text: String = chars[s.start..s.end.min(chars.len())].iter().collect();
                Segment::new(&text, &s.style)
            })
            .collect()
    }

    #[test]
    fn snapshot_zsh_echo_hello() {
        let e = engine();
        let source = "echo hello";
        let spans = e.highlight(LanguageConfig::Zsh, source).unwrap();
        assert_that!(
            spans_to_segments(&spans, source),
            eq(&[Segment::new("echo", "fg=#7aa2f7")])
        );
    }

    #[test]
    fn snapshot_zsh_combined_static_dynamic() {
        let e = HighlightEngine::new(tokyonight_dark()).unwrap();
        let source = "echo hello";
        let spans = e.highlight(LanguageConfig::Zsh, source).unwrap();
        insta::assert_snapshot!(fmt_spans(&spans, source));
    }

    #[test]
    fn snapshot_zsh_comment() {
        let e = engine();
        let source = "# foo bar";
        let spans = e.highlight(LanguageConfig::Zsh, source).unwrap();
        insta::assert_snapshot!(fmt_spans(&spans, source));
    }

    #[test]
    fn snapshot_zsh_quoted_string() {
        let e = engine();
        let source = r#"echo "hello world""#;
        let spans = e.highlight(LanguageConfig::Zsh, source).unwrap();
        insta::assert_snapshot!(fmt_spans(&spans, source));
    }

    #[test]
    fn snapshot_zsh_variable() {
        let e = engine();
        let source = "echo $HOME";
        let spans = e.highlight(LanguageConfig::Zsh, source).unwrap();
        insta::assert_snapshot!(fmt_spans(&spans, source));
    }

    #[test]
    fn snapshot_zsh_subshell() {
        let e = engine();
        let source = "echo $(date)";
        let spans = e.highlight(LanguageConfig::Zsh, source).unwrap();
        insta::assert_snapshot!(fmt_spans(&spans, source));
    }

    #[test]
    fn snapshot_zsh_pipe_redir() {
        let e = engine();
        let source = "cat file | grep foo > out.txt";
        let spans = e.highlight(LanguageConfig::Zsh, source).unwrap();
        insta::assert_snapshot!(fmt_spans(&spans, source));
    }

    #[test]
    fn snapshot_md_heading() {
        let e = engine();
        let source = "# Hello\n";
        let spans = e.highlight(LanguageConfig::Markdown, source).unwrap();
        insta::assert_snapshot!(fmt_spans(&spans, source));
    }

    #[test]
    fn snapshot_md_bold() {
        let e = engine();
        let source = "**hello**\n";
        let spans = e.highlight(LanguageConfig::Markdown, source).unwrap();
        insta::assert_snapshot!(fmt_spans(&spans, source));
    }

    #[test]
    fn snapshot_md_italic() {
        let e = engine();
        let source = "*hello*\n";
        let spans = e.highlight(LanguageConfig::Markdown, source).unwrap();
        insta::assert_snapshot!(fmt_spans(&spans, source));
    }

    #[test]
    fn snapshot_md_inline_code() {
        let e = engine();
        let source = "use `cmd` here\n";
        let spans = e.highlight(LanguageConfig::Markdown, source).unwrap();
        insta::assert_snapshot!(fmt_spans(&spans, source));
    }

    #[test]
    fn snapshot_md_code_fence_zsh() {
        let e = engine();
        let source = "```zsh\necho hi\n```\n";
        let spans = e.highlight(LanguageConfig::Markdown, source).unwrap();
        insta::assert_snapshot!(fmt_spans(&spans, source));
    }

    #[test]
    fn snapshot_md_link() {
        let e = engine();
        let source = "[link](https://example.com)\n";
        let spans = e.highlight(LanguageConfig::Markdown, source).unwrap();
        insta::assert_snapshot!(fmt_spans(&spans, source));
    }

    #[test]
    fn snapshot_md_unordered_list() {
        let e = engine();
        let source = "- foo\n- bar\n";
        let spans = e.highlight(LanguageConfig::Markdown, source).unwrap();
        insta::assert_snapshot!(fmt_spans(&spans, source));
    }

    #[test]
    fn snapshot_md_task_list() {
        let e = engine();
        let source = "- [x] done\n- [ ] todo\n";
        let spans = e.highlight(LanguageConfig::Markdown, source).unwrap();
        insta::assert_snapshot!(fmt_spans(&spans, source));
    }

    #[test]
    fn highlight_md_code_block_with_zsh_injection() {
        let e = engine();
        let source = "```zsh\necho hi\n```";
        let spans = e.highlight(LanguageConfig::Markdown, source).unwrap();
        // Should contain zsh-style highlighting inside the code block
        assert!(!spans.is_empty());
    }

    #[test]
    fn all_capture_names_have_theme_entries() {
        let e = engine();
        let names = e.all_capture_names();
        let theme = tokyonight_dark();
        let mut missing = Vec::new();
        for name in &names {
            let has_match = theme.names().iter().any(|tname| {
                let parts: Vec<_> = tname.split('.').collect();
                parts.iter().all(|p| name.split('.').any(|np| np == *p))
            });
            if !has_match {
                missing.push(name.clone());
            }
        }
        assert!(missing.is_empty(), "theme missing captures: {:?}", missing);
    }
}

//! Incremental re-parsing for a buffer that only arrives as snapshots.
//!
//! ZLE hands us the whole `BUFFER` on every redraw, never the edit that produced
//! it. We recover that edit by diffing against the previous snapshot (common
//! prefix and suffix), mark it on the old tree, and let tree-sitter reuse every
//! subtree the edit did not touch.

use anyhow::{Context, Result};

/// A parser that keeps the last tree and source so the next parse is incremental.
pub struct IncrementalParser {
    parser: tree_sitter::Parser,
    tree: Option<tree_sitter::Tree>,
    source: String,
}

impl IncrementalParser {
    pub fn new(language: &tree_sitter::Language) -> Result<Self> {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(language)?;
        Ok(Self {
            parser,
            tree: None,
            source: String::new(),
        })
    }

    /// Parse `source`, reusing the previous tree when there is one.
    pub fn parse(&mut self, source: &str) -> Result<&tree_sitter::Tree> {
        if self.tree.is_none() || self.source != source {
            self.reparse(source)?;
        }
        Ok(self.tree.as_ref().expect("reparse sets the tree"))
    }

    fn reparse(&mut self, source: &str) -> Result<()> {
        // Taken so a failed parse leaves no tree that disagrees with `self.source`.
        let mut old = self.tree.take();
        if let Some(tree) = old.as_mut() {
            tree.edit(&input_edit(&self.source, source));
        }
        let tree = self
            .parser
            .parse(source, old.as_ref())
            .context("zsh parsing failed")?;
        self.source.clear();
        self.source.push_str(source);
        self.tree = Some(tree);
        Ok(())
    }
}

/// The single edit that turns `old` into `new`: everything between their common
/// prefix and common suffix was replaced.
pub fn input_edit(old: &str, new: &str) -> tree_sitter::InputEdit {
    let mut prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    // The prefix is shared, so a char boundary in one is a boundary in the other.
    while !old.is_char_boundary(prefix) {
        prefix -= 1;
    }

    let max_suffix = old.len().min(new.len()) - prefix;
    let mut suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(old.len() - suffix) {
        suffix -= 1;
    }

    let old_end = old.len() - suffix;
    let new_end = new.len() - suffix;
    tree_sitter::InputEdit {
        start_byte: prefix,
        old_end_byte: old_end,
        new_end_byte: new_end,
        start_position: point_at(old, prefix),
        old_end_position: point_at(old, old_end),
        new_end_position: point_at(new, new_end),
    }
}

/// The `(row, byte column)` of `byte` in `text`.
fn point_at(text: &str, byte: usize) -> tree_sitter::Point {
    let before = &text.as_bytes()[..byte];
    let row = before.iter().filter(|&&b| b == b'\n').count();
    let line_start = before
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    tree_sitter::Point {
        row,
        column: byte - line_start,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zsh() -> tree_sitter::Language {
        tree_sitter_zsh::LANGUAGE.into()
    }

    fn cold_sexp(source: &str) -> String {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&zsh()).unwrap();
        parser.parse(source, None).unwrap().root_node().to_sexp()
    }

    fn edit_bytes(e: &tree_sitter::InputEdit) -> (usize, usize, usize) {
        (e.start_byte, e.old_end_byte, e.new_end_byte)
    }

    #[test]
    fn edit_for_append() {
        assert_eq!(edit_bytes(&input_edit("echo", "echo hi")), (4, 4, 7));
    }

    #[test]
    fn edit_for_delete_at_end() {
        assert_eq!(edit_bytes(&input_edit("echo hi", "echo")), (4, 7, 4));
    }

    #[test]
    fn edit_for_middle_insert() {
        assert_eq!(edit_bytes(&input_edit("ab", "aXb")), (1, 1, 2));
    }

    #[test]
    fn edit_for_identical_text_is_empty() {
        assert_eq!(edit_bytes(&input_edit("same", "same")), (4, 4, 4));
    }

    #[test]
    fn edit_for_repeated_chars_does_not_overlap() {
        // "aaa" -> "aaaa": prefix and suffix must not both claim the same bytes.
        assert_eq!(edit_bytes(&input_edit("aaa", "aaaa")), (3, 3, 4));
        assert_eq!(edit_bytes(&input_edit("aaaa", "aaa")), (3, 4, 3));
    }

    #[test]
    fn edit_snaps_to_char_boundaries() {
        // é (c3 a9) -> è (c3 a8): shared lead byte must not split the char.
        let e = input_edit("aéb", "aèb");
        assert_eq!(edit_bytes(&e), (1, 3, 3));
        // ü (c3 bc) vs ö (c3 b6) after a shared suffix byte.
        let e = input_edit("xü", "xö");
        assert_eq!(edit_bytes(&e), (1, 3, 3));
    }

    #[test]
    fn edit_points_are_byte_columns_on_later_lines() {
        let e = input_edit("ls\necho é", "ls\necho éX");
        assert_eq!(e.start_position, tree_sitter::Point { row: 1, column: 7 });
        assert_eq!(e.new_end_position, tree_sitter::Point { row: 1, column: 8 });
    }

    #[test]
    fn incremental_parse_matches_cold_parse_while_typing() {
        let target = "for f in ./src/*.rs; do\n  echo \"$f é\" | sed 's/a/b/' >> /tmp/x\ndone";
        let mut p = IncrementalParser::new(&zsh()).unwrap();
        let mut typed = String::new();
        for ch in target.chars() {
            typed.push(ch);
            let sexp = p.parse(&typed).unwrap().root_node().to_sexp();
            assert_eq!(sexp, cold_sexp(&typed), "after typing {typed:?}");
        }
        // Then backspace it all away.
        while typed.pop().is_some() {
            let sexp = p.parse(&typed).unwrap().root_node().to_sexp();
            assert_eq!(sexp, cold_sexp(&typed), "after deleting to {typed:?}");
        }
    }

    #[test]
    fn incremental_parse_matches_cold_parse_for_arbitrary_jumps() {
        let snapshots = [
            "echo hello",
            "echo \"hello", // unterminated string
            "echo \"hello\" && ls",
            "if true; then\n  ls\nfi",
            "cat <<EOF\nbody\nEOF\n", // heredoc (external scanner)
            "",
            "foo() { bar; }; foo",
            "echo hello",
        ];
        let mut p = IncrementalParser::new(&zsh()).unwrap();
        for s in snapshots {
            let sexp = p.parse(s).unwrap().root_node().to_sexp();
            assert_eq!(sexp, cold_sexp(s), "for {s:?}");
        }
    }
}

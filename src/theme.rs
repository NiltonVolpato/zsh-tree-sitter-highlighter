use std::collections::HashMap;

/// An ANSI style for Zsh's `region_highlight`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

impl Style {
    pub fn new() -> Self {
        Self {
            foreground: None,
            background: None,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        }
    }

    pub fn fg(mut self, color: impl Into<String>) -> Self {
        self.foreground = Some(color.into());
        self
    }

    pub fn bg(mut self, color: impl Into<String>) -> Self {
        self.background = Some(color.into());
        self
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    #[cfg(test)]
    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }

    /// Render as a Zsh `region_highlight` attribute string.
    pub fn to_ansi(&self) -> String {
        let mut parts = Vec::new();
        if let Some(fg) = &self.foreground {
            parts.push(format!("fg={fg}"));
        }
        if let Some(bg) = &self.background {
            parts.push(format!("bg={bg}"));
        }
        if self.bold {
            parts.push("bold".to_string());
        }
        if self.italic {
            parts.push("italic".to_string());
        }
        if self.underline {
            parts.push("underline".to_string());
        }
        if self.strikethrough {
            parts.push("strikethrough".to_string());
        }
        parts.join(",")
    }
}

impl Default for Style {
    fn default() -> Self {
        Self::new()
    }
}

/// Maps tree-sitter capture names to `Style`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    styles: HashMap<String, Style>,
    names: Vec<String>,
}

impl Theme {
    pub fn new() -> Self {
        Self {
            styles: HashMap::new(),
            names: Vec::new(),
        }
    }

    pub fn insert(&mut self, name: impl Into<String>, style: Style) {
        let name = name.into();
        if !self.styles.contains_key(&name) {
            self.names.push(name.clone());
        }
        self.styles.insert(name, style);
    }

    #[cfg(test)]
    pub fn get(&self, name: &str) -> Option<&Style> {
        self.styles.get(name)
    }

    /// Ordered list of capture names for `HighlightConfiguration::configure()`.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Lookup by highlight index (as returned by `configure()`).
    pub fn style_by_index(&self, index: usize) -> Option<&Style> {
        self.names.get(index).and_then(|n| self.styles.get(n))
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::new()
    }
}

/// Hardcoded Tokyonight Dark theme.
pub fn tokyonight_dark() -> Theme {
    let mut theme = Theme::new();

    // Zsh captures
    theme.insert("keyword", Style::new().fg("#c099ff"));
    theme.insert("string", Style::new().fg("#e0af68"));
    theme.insert("comment", Style::new().fg("#565f89"));
    theme.insert("function", Style::new().fg("#7aa2f7"));
    theme.insert("property", Style::new().fg("#9ece6a"));
    theme.insert("number", Style::new().fg("#ff9e64"));
    theme.insert("operator", Style::new().fg("#7882bf"));
    theme.insert("constant", Style::new().fg("#ff9e64"));
    theme.insert("embedded", Style::new().fg("#73daca"));

    // Markdown / common captures
    theme.insert("punctuation", Style::new().fg("#7882bf"));
    theme.insert("punctuation.bracket", Style::new().fg("#565f89"));
    theme.insert("punctuation.delimiter", Style::new().fg("#7882bf"));
    theme.insert("punctuation.special", Style::new().fg("#ff9e64"));

    theme.insert("text.title", Style::new().fg("#7aa2f7").bold());
    theme.insert("text.literal", Style::new().fg("#e0af68"));
    theme.insert("text.emphasis", Style::new().italic());
    theme.insert("text.strong", Style::new().bold());
    theme.insert("text.uri", Style::new().fg("#1abc9c").underline());
    theme.insert("text.reference", Style::new().fg("#1abc9c"));
    theme.insert("string.escape", Style::new().fg("#bb9af7"));
    theme.insert("none", Style::new());

    // Internal injection captures used by tree-sitter-highlight
    theme.insert("injection.content", Style::new());
    theme.insert("injection.language", Style::new());

    // Markup aliases (for standard tree-sitter-highlight names if queries ever change)
    theme.insert("markup.heading", Style::new().fg("#7aa2f7").bold());
    theme.insert("markup.strong", Style::new().bold());
    theme.insert("markup.italic", Style::new().italic());
    theme.insert("markup.link", Style::new().fg("#1abc9c").underline());
    theme.insert("markup.raw", Style::new().fg("#e0af68"));
    theme.insert("markup.raw.inline", Style::new().fg("#7aa2f7").bg("#414868"));
    theme.insert("markup.list", Style::new().fg("#ff9e64"));
    theme.insert("markup.list.checked", Style::new().fg("#9ece6a"));
    theme.insert("markup.list.unchecked", Style::new().fg("#7aa2f7"));

    // Variable / type fallbacks
    theme.insert("variable", Style::new().fg("#c0caf5"));
    theme.insert("variable.builtin", Style::new().fg("#f7768e"));
    theme.insert("type", Style::new().fg("#2ac3de"));
    theme.insert("type.builtin", Style::new().fg("#2ac3de"));

    theme
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_to_ansi_full() {
        let s = Style::new()
            .fg("#c099ff")
            .bg("#1a1b26")
            .bold()
            .italic()
            .underline()
            .strikethrough();
        assert_eq!(
            s.to_ansi(),
            "fg=#c099ff,bg=#1a1b26,bold,italic,underline,strikethrough"
        );
    }

    #[test]
    fn style_to_ansi_empty() {
        assert_eq!(Style::new().to_ansi(), "");
    }

    #[test]
    fn tokyonight_has_keyword() {
        let t = tokyonight_dark();
        let s = t.get("keyword").unwrap();
        assert_eq!(s.foreground, Some("#c099ff".to_string()));
        assert!(!s.bold);
    }

    #[test]
    fn tokyonight_has_markup_heading() {
        let t = tokyonight_dark();
        let s = t.get("markup.heading").unwrap();
        assert_eq!(s.foreground, Some("#7aa2f7".to_string()));
        assert!(s.bold);
    }

    #[test]
    fn tokyonight_theme_covers_all_query_captures() {
        let t = tokyonight_dark();
        let required = vec![
            "keyword", "string", "comment", "function", "property", "number",
            "operator", "constant", "embedded", "punctuation", "punctuation.bracket",
            "punctuation.delimiter", "punctuation.special", "text.title",
            "text.literal", "text.emphasis", "text.strong", "text.uri",
            "text.reference", "string.escape", "none", "markup.heading",
            "markup.strong", "markup.italic", "markup.link", "markup.raw",
            "markup.raw.inline", "markup.list", "markup.list.checked",
            "markup.list.unchecked", "variable", "variable.builtin", "type",
            "type.builtin",
        ];
        for name in required {
            assert!(t.get(name).is_some(), "theme missing capture: {name}");
        }
    }
}

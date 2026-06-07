use std::collections::HashMap;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
struct Theme {
    inherits: Option<String>,
    #[serde(default)]
    palette: HashMap<String, String>,
    #[serde(flatten)]
    styles: HashMap<String, StyleValue>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
#[allow(dead_code)]
enum StyleValue {
    Simple(String),
    Full(Style),
    PaletteList(Vec<String>),
}

#[derive(Deserialize, Debug, Clone, Default)]
struct Style {
    fg: Option<String>,
    bg: Option<String>,
    modifiers: Option<Vec<String>>,
    underline: Option<toml::Value>,
}

fn resolve_color(color: &str, palette: &HashMap<String, String>) -> String {
    if color.starts_with('#') {
        color.to_string()
    } else if let Some(resolved) = palette.get(color) {
        resolved.to_string()
    } else {
        color.to_string()
    }
}

fn translate_style(value: &StyleValue, palette: &HashMap<String, String>) -> Option<String> {
    match value {
        StyleValue::Simple(color) => {
            let resolved = resolve_color(color, palette);
            Some(format!("fg={}", resolved))
        }
        StyleValue::Full(style) => {
            let mut parts = Vec::new();
            if let Some(ref fg) = style.fg {
                let resolved = resolve_color(fg, palette);
                parts.push(format!("fg={}", resolved));
            }
            if let Some(ref bg) = style.bg {
                let resolved = resolve_color(bg, palette);
                parts.push(format!("bg={}", resolved));
            }
            if let Some(ref mods) = style.modifiers {
                for m in mods {
                    match m.as_str() {
                        "bold" => parts.push("bold".to_string()),
                        "italic" => parts.push("italic".to_string()),
                        "underlined" | "underline" => parts.push("underline".to_string()),
                        "dim" => parts.push("dim".to_string()),
                        _ => {}
                    }
                }
            }
            if style.underline.is_some() {
                if !parts.contains(&"underline".to_string()) {
                    parts.push("underline".to_string());
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(","))
            }
        }
        StyleValue::PaletteList(_) => None,
    }
}

fn get_style(
    capture_name: &str,
    styles: &HashMap<String, StyleValue>,
    palette: &HashMap<String, String>,
) -> Option<String> {
    // Look up directly in styles
    if let Some(val) = styles.get(capture_name) {
        if let Some(style_str) = translate_style(val, palette) {
            return Some(style_str);
        }
    }

    // Handle custom/override fallbacks if not resolved by theme
    match capture_name {
        "command.invalid" => {
            if let Some(val) = styles.get("error") {
                translate_style(val, palette)
            } else {
                None
            }
        }
        "path" => {
            Some("underline".to_string())
        }
        "path.directory" => {
            let mut parts = vec!["underline".to_string()];
            for dir_key in &["ui.text.directory", "function", "blue"] {
                if let Some(val) = styles.get(*dir_key) {
                    if let Some(style_str) = translate_style(val, palette) {
                        for component in style_str.split(',') {
                            if component.starts_with("fg=") {
                                parts.push(component.to_string());
                                break;
                            }
                        }
                        break;
                    }
                }
            }
            Some(parts.join(","))
        }
        _ => None,
    }
}

fn load_theme_recursive(path: &std::path::Path, depth: usize) -> Result<Theme, String> {
    if depth > 10 {
        return Err("theme inheritance depth exceeded limit of 10".to_string());
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read theme file '{}': {}", path.display(), e))?;

    let mut theme: Theme = toml::from_str(&content)
        .map_err(|e| format!("failed to parse theme file '{}': {}", path.display(), e))?;

    if let Some(ref parent_name) = theme.inherits {
        let parent_path = path
            .parent()
            .ok_or_else(|| format!("failed to get parent directory of '{}'", path.display()))?
            .join(format!("{}.toml", parent_name));

        let parent_theme = load_theme_recursive(&parent_path, depth + 1)?;

        // Merge parent theme's palette and styles into the current theme.
        // The child's values should override the parent's values.
        let mut merged_palette = parent_theme.palette;
        for (k, v) in theme.palette {
            merged_palette.insert(k, v);
        }
        theme.palette = merged_palette;

        let mut merged_styles = parent_theme.styles;
        for (k, v) in theme.styles {
            merged_styles.insert(k, v);
        }
        theme.styles = merged_styles;
    }

    Ok(theme)
}

pub fn load_theme_file(path: &str) -> Result<HashMap<String, String>, String> {
    let path_buf = std::path::Path::new(path);
    let theme = load_theme_recursive(path_buf, 0)?;

    let mut cache = HashMap::new();
    let all_captures = [
        "comment",
        "constant",
        "embedded",
        "function",
        "keyword",
        "number",
        "operator",
        "property",
        "punctuation.delimiter",
        "punctuation.special",
        "string",
        "string.escape",
        "text.emphasis",
        "text.literal",
        "text.reference",
        "text.strong",
        "text.title",
        "text.uri",
        "variable",
        "command.invalid",
        "path",
        "path.directory",
    ];

    for capture in all_captures {
        if let Some(style_str) = get_style(capture, &theme.styles, &theme.palette) {
            cache.insert(capture.to_string(), style_str);
        }
    }

    Ok(cache)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_color() {
        let mut palette = HashMap::new();
        palette.insert("red".to_string(), "#ff0000".to_string());

        assert_eq!(resolve_color("#00ff00", &palette), "#00ff00");
        assert_eq!(resolve_color("red", &palette), "#ff0000");
        assert_eq!(resolve_color("blue", &palette), "blue");
    }

    #[test]
    fn test_load_onedark() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let path = std::path::Path::new(&manifest_dir).join("themes/onedark.toml");
        let cache = load_theme_file(path.to_str().unwrap()).unwrap();

        assert_eq!(cache.get("comment").unwrap(), "fg=#5C6370,italic");
        assert_eq!(cache.get("function").unwrap(), "fg=#61AFEF");
        assert_eq!(cache.get("command.invalid").unwrap(), "fg=#E06C75,bold");
        assert_eq!(cache.get("path").unwrap(), "underline");
        assert_eq!(cache.get("path.directory").unwrap(), "underline,fg=#61AFEF");
    }

    #[test]
    fn test_load_inherited_theme() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let path = std::path::Path::new(&manifest_dir).join("themes/catppuccin_frappe.toml");
        let cache = load_theme_file(path.to_str().unwrap()).unwrap();

        // comment should be inherited from catppuccin_mocha
        // and resolve using catppuccin_frappe's overlay2 color (#949cbb)
        assert_eq!(cache.get("comment").unwrap(), "fg=#949cbb,italic");
    }

    #[test]
    fn test_essential_themes_define_all_captures() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let essential_themes = ["onedark.toml", "tokyonight.toml"];

        let all_captures = [
            "comment",
            "constant",
            "embedded",
            "function",
            "keyword",
            "number",
            "operator",
            "property",
            "punctuation.delimiter",
            "punctuation.special",
            "string",
            "string.escape",
            "text.emphasis",
            "text.literal",
            "text.reference",
            "text.strong",
            "text.title",
            "text.uri",
            "variable",
        ];

        for theme_name in &essential_themes {
            let path = std::path::Path::new(&manifest_dir).join("themes").join(theme_name);
            let cache = load_theme_file(path.to_str().unwrap())
                .unwrap_or_else(|e| panic!("failed to load theme '{}': {}", theme_name, e));

            for capture in &all_captures {
                assert!(
                    cache.contains_key(*capture),
                    "Theme '{}' is missing mapping for semantic capture '{}'",
                    theme_name,
                    capture
                );
            }
        }
    }

    #[test]
    fn test_distinct_colors_for_adjacent_elements() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let essential_themes = ["onedark.toml", "tokyonight.toml"];

        for theme_name in &essential_themes {
            let path = std::path::Path::new(&manifest_dir).join("themes").join(theme_name);
            let cache = load_theme_file(path.to_str().unwrap()).unwrap();

            // Quality Check: 'function' (commands) and 'embedded' (subshells) should not share the same color.
            let func_style = cache.get("function");
            let embed_style = cache.get("embedded");
            if let (Some(f), Some(e)) = (func_style, embed_style) {
                assert_ne!(
                    f, e,
                    "Theme '{}' has overlapping styles for 'function' and 'embedded': '{}'",
                    theme_name, f
                );
            }
        }
    }
}

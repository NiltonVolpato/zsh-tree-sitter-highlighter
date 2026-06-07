use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone)]
struct Theme {
    inherits: Option<String>,
    #[serde(default)]
    palette: HashMap<String, String>,
    #[serde(flatten)]
    styles: HashMap<String, StyleValue>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
enum StyleValue {
    Simple(String),
    Full(Style),
    PaletteList(Vec<String>),
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
struct Style {
    #[serde(skip_serializing_if = "Option::is_none")]
    fg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    modifiers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    underline: Option<toml::Value>,
}

fn load_theme_recursive(path: &Path, depth: usize) -> Result<Theme, String> {
    if depth > 10 {
        return Err("theme inheritance depth exceeded limit of 10".to_string());
    }

    let content = fs::read_to_string(path)
        .map_err(|e| format!("failed to read theme file '{}': {}", path.display(), e))?;

    let mut theme: Theme = toml::from_str(&content)
        .map_err(|e| format!("failed to parse theme file '{}': {}", path.display(), e))?;

    if let Some(ref parent_name) = theme.inherits {
        let parent_path = path
            .parent()
            .ok_or_else(|| format!("failed to get parent directory of '{}'", path.display()))?
            .join(format!("{}.toml", parent_name));

        let parent_theme = load_theme_recursive(&parent_path, depth + 1)?;

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

fn get_lookup_keys(capture_name: &str) -> Vec<String> {
    let mut keys = Vec::new();
    keys.push(capture_name.to_string());

    match capture_name {
        "text.title" => {
            keys.push("markup.heading".to_string());
            keys.push("markup.heading.1".to_string());
        }
        "text.strong" => {
            keys.push("markup.bold".to_string());
        }
        "text.emphasis" => {
            keys.push("markup.italic".to_string());
        }
        "text.literal" => {
            keys.push("markup.raw".to_string());
        }
        "text.reference" => {
            keys.push("markup.link.text".to_string());
        }
        "text.uri" => {
            keys.push("markup.link.url".to_string());
        }
        "punctuation.delimiter" => {
            keys.push("punctuation".to_string());
        }
        "punctuation.special" => {
            keys.push("special".to_string());
            keys.push("punctuation".to_string());
        }
        "string.escape" => {
            keys.push("constant.character.escape".to_string());
            keys.push("constant".to_string());
        }
        "embedded" => {
            keys.push("special".to_string());
        }
        "property" => {
            keys.push("variable".to_string());
        }
        "number" => {
            keys.push("constant".to_string());
        }
        _ => {}
    }
    keys
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: theme-converter <input_helix_theme.toml> <output_theme.toml>");
        std::process::exit(1);
    }

    let input_path = PathBuf::from(&args[1]);
    let output_path = PathBuf::from(&args[2]);

    // 1. Load the theme with full inheritance resolved
    let theme = load_theme_recursive(&input_path, 0)?;

    // 2. Perform fuzzy lookups for all required captures
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

    let mut output_styles = HashMap::new();
    for capture in &all_captures {
        let keys = get_lookup_keys(capture);
        for key in keys {
            if let Some(val) = theme.styles.get(&key) {
                output_styles.insert(capture.to_string(), val.clone());
                break;
            }
        }
    }

    // Also include custom overrides if defined
    if let Some(val) = theme.styles.get("error") {
        output_styles.insert("command.invalid".to_string(), val.clone());
    }
    if let Some(val) = theme.styles.get("ui.text.directory") {
        output_styles.insert("path.directory".to_string(), val.clone());
    }

    // 3. Serialize to TOML, preserving palette block
    let mut output_theme_toml = String::new();
    
    // Write inherits if it was defined
    if let Some(ref inherits) = theme.inherits {
        output_theme_toml.push_str(&format!("inherits = \"{}\"\n\n", inherits));
    }

    // Write palette block if it was defined
    if !theme.palette.is_empty() {
        output_theme_toml.push_str("[palette]\n");
        let mut sorted_palette: Vec<_> = theme.palette.iter().collect();
        sorted_palette.sort_by_key(|(k, _)| *k);
        for (k, v) in sorted_palette {
            output_theme_toml.push_str(&format!("{} = \"{}\"\n", k, v));
        }
        output_theme_toml.push_str("\n");
    }

    // Write the flat styles matching our 1-to-1 keys
    let mut sorted_styles: Vec<_> = output_styles.iter().collect();
    sorted_styles.sort_by_key(|(k, _)| *k);
    for (k, val) in sorted_styles {
        let val_str = toml::to_string(val)?;
        output_theme_toml.push_str(&format!("\"{}\" = {}\n", k, val_str.trim()));
    }

    fs::write(&output_path, output_theme_toml)?;
    println!("Successfully converted '{}' to '{}'", input_path.display(), output_path.display());
    Ok(())
}

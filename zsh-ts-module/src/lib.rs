use std::collections::HashMap;
use std::ffi::{CStr, CString};

use zsh_module::{builtin, env, flags, state, Activate, Deactivate, Result};
use zsh_module::__::zsh as zsh_sys;
use zsh_tree_sitter_highlighter::highlight::{HighlightEngine, LanguageConfig, Span};

// ---------------------------------------------------------------------------
// Direct O(1) hash table lookups via zsh-sys
// ---------------------------------------------------------------------------

fn is_in_hashtable(table: zsh_sys::HashTable, name: &str) -> bool {
    let cname = match CString::new(name) {
        Ok(s) => s,
        Err(_) => return false,
    };
    unsafe {
        let metafied = zsh_sys::ztrdup_metafy(cname.as_ptr());
        if metafied.is_null() {
            return false;
        }
        let node = (*table).getnode.unwrap()(table, metafied);
        zsh_sys::zsfree(metafied);
        !node.is_null()
    }
}

fn is_command(name: &str) -> bool {
    unsafe { is_in_hashtable(zsh_sys::cmdnamtab, name) }
}

fn is_function(name: &str) -> bool {
    unsafe { is_in_hashtable(zsh_sys::shfunctab, name) }
}

fn is_builtin(name: &str) -> bool {
    unsafe { is_in_hashtable(zsh_sys::builtintab, name) }
}

fn is_alias(name: &str) -> bool {
    unsafe { is_in_hashtable(zsh_sys::aliastab, name) }
}

fn is_valid_command(name: &str) -> bool {
    is_command(name) || is_function(name) || is_builtin(name) || is_alias(name)
}

// ---------------------------------------------------------------------------
// Module state
// ---------------------------------------------------------------------------

#[state]
#[derive(Default)]
struct HighlighterState {
    engine: std::panic::AssertUnwindSafe<Option<HighlightEngine>>,
}

impl std::fmt::Debug for HighlighterState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HighlighterState").finish()
    }
}

impl Activate for HighlighterState {
    fn activate(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Deactivate for HighlighterState {
    fn deactivate(&mut self) -> Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Builtin: zsh_ts_highlight
// ---------------------------------------------------------------------------

#[builtin("zsh_ts_highlight", min = 0, max = 0, opts = "")]
fn zsh_ts_highlight(
    state: &mut HighlighterState,
    _name: &CStr,
    _args: &[&CStr],
    _opts: &flags::Flags,
) -> Result<()> {
    // Lazy-initialize the highlight engine on first call.
    let engine = match state.engine.as_mut() {
        Some(e) => e,
        None => {
            let e = match HighlightEngine::new() {
                Ok(e) => e,
                Err(err) => {
                    let _ = env::set(
                        "_zsh_ts_error",
                        format!("engine init failed: {}", err),
                    );
                    return Ok(());
                }
            };
            *state.engine = Some(e);
            state.engine.as_mut().unwrap()
        }
    };

    // Read ZLE and environment state.
    let buffer = env::get::<String>("BUFFER").unwrap_or_default();
    let prebuffer = env::get::<String>("PREBUFFER").unwrap_or_default();
    let pwd = env::get::<String>("PWD").ok();

    // Determine language mode.
    let mode = env::get::<String>("ZSH_TS_HIGHLIGHTER_MODE").unwrap_or_else(|_| "zsh".into());
    let language = match mode.as_str() {
        "md" | "markdown" => LanguageConfig::Markdown,
        _ => LanguageConfig::Zsh,
    };

    // Read theme associative array.
    let theme = match env::get::<HashMap<String, String>>("_ZSH_TS_HIGHLIGHTER_THEME") {
        Ok(t) => t,
        Err(_) => {
            let _ = env::set(
                "_zsh_ts_error",
                "_ZSH_TS_HIGHLIGHTER_THEME not set (did you source activate.zsh?)",
            );
            return Ok(());
        }
    };
    if theme.is_empty() {
        let _ = env::set(
            "_zsh_ts_error",
            "_ZSH_TS_HIGHLIGHTER_THEME is empty (did you source activate.zsh?)",
        );
        return Ok(());
    }

    let full_source = format!("{}{}", prebuffer, buffer);
    let prebuffer_char_len = prebuffer.chars().count();

    // Run tree-sitter highlighting (static + dynamic path resolution).
    let pwd_str = pwd.as_deref();
    let mut spans = match engine.highlight_with_pwd(language, &full_source, pwd_str) {
        Ok(s) => s,
        Err(err) => {
            let _ = env::set("_zsh_ts_error", format!("highlight failed: {}", err));
            return Ok(());
        }
    };

    // Validate commands and add override spans for invalid ones (zsh mode only).
    if language == LanguageConfig::Zsh {
        match engine.extract_zsh_commands(&full_source) {
            Ok(commands) => {
                for (start, end, name) in commands {
                    if !is_valid_command(&name) {
                        spans.push(Span {
                            start,
                            end,
                            style: "command.invalid".to_string(),
                        });
                    }
                }
            }
            Err(err) => {
                let _ = env::set(
                    "_zsh_ts_error",
                    format!("command extraction failed: {}", err),
                );
                return Ok(());
            }
        }
    }

    // Sort: by start position, and for equal starts longer spans first so that
    // "last wins" gives precedence to more specific spans.
    spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));

    // Shift coordinates from full_source to buffer-only, filter prebuffer spans,
    // and map capture names to theme attributes.
    let mut regions: Vec<String> = Vec::new();
    for span in spans {
        if span.end <= prebuffer_char_len {
            continue;
        }
        let start = if span.start < prebuffer_char_len {
            0
        } else {
            span.start - prebuffer_char_len
        };
        let end = span.end - prebuffer_char_len;
        if start >= end {
            continue;
        }
        if let Some(style) = theme.get(&span.style) {
            regions.push(format!("{} {} {}", start, end, style));
        }
    }

    // Write the result array for the zsh adapter to pick up.
    if let Err(e) = env::set("_zsh_ts_regions", regions) {
        let _ = env::set("_zsh_ts_error", format!("failed to set regions: {:?}", e));
    }

    Ok(())
}

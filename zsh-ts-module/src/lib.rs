use zsh_module::zsh_module;

#[zsh_module]
mod zsh_ts_module {
    use std::collections::HashMap;

    use zsh_module::env::ParamSetValue;
    use zsh_module::{Association, EnvAccess, Termination};
    use zsh_tree_sitter_highlighter::highlight::{HighlightEngine, LanguageConfig, Span};

    // ---------------------------------------------------------------------------
    // Helper: validate commands using the high-level Zsh table APIs
    // ---------------------------------------------------------------------------

    fn is_valid_command(env: &zsh_module::Env, name: &str) -> bool {
        env.commands().contains_key(name)
            || env.functions().contains_key(name)
            || env.builtins().contains_key(name)
            || env.aliases().contains_key(name)
    }

    // ---------------------------------------------------------------------------
    // Module state & Component
    // ---------------------------------------------------------------------------

    #[zsh_component]
    #[derive(Default)]
    pub struct Highlighter {
        engine: Option<HighlightEngine>,
    }

    impl std::fmt::Debug for Highlighter {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Highlighter").finish()
        }
    }

    impl Highlighter {
        pub fn new() -> Self {
            Self::default()
        }

        // ---------------------------------------------------------------------------
        // Builtin: zsh_ts_highlight
        // ---------------------------------------------------------------------------

        pub fn zsh_ts_highlight(&mut self, _args: &[&str]) -> Result<(), Termination> {
            let env = self.env();

            // Lazy-initialize the highlight engine on first call.
            let engine = match self.engine.as_mut() {
                Some(e) => e,
                None => {
                    let e = match HighlightEngine::new() {
                        Ok(e) => e,
                        Err(err) => {
                            let _ = env.set(
                                "_zsh_ts_error",
                                ParamSetValue::Scalar(&format!("engine init failed: {}", err)),
                            );
                            return Ok(());
                        }
                    };
                    self.engine = Some(e);
                    self.engine.as_mut().unwrap()
                }
            };

            // Read ZLE and environment state.
            let buffer = env
                .get("BUFFER")
                .and_then(|p| p.as_scalar().map(|s| s.into_owned()))
                .ok()
                .unwrap_or_default();
            let prebuffer = env
                .get("PREBUFFER")
                .and_then(|p| p.as_scalar().map(|s| s.into_owned()))
                .ok()
                .unwrap_or_default();
            let pwd = env
                .get("PWD")
                .and_then(|p| p.as_scalar().map(|s| s.into_owned()))
                .ok();

            // Determine language mode.
            let mode = env
                .get("ZSH_TS_HIGHLIGHTER_MODE")
                .and_then(|p| p.as_scalar().map(|s| s.into_owned()))
                .ok()
                .unwrap_or_else(|| "zsh".into());
            let language = match mode.as_str() {
                "md" | "markdown" => LanguageConfig::Markdown,
                _ => LanguageConfig::Zsh,
            };

            // Read theme associative array.
            let theme_param = match env.get("_ZSH_TS_HIGHLIGHTER_THEME") {
                Ok(p) => p,
                Err(_) => {
                    let _ = env.set(
                        "_zsh_ts_error",
                        ParamSetValue::Scalar(
                            "_ZSH_TS_HIGHLIGHTER_THEME not set (did you source activate.zsh?)",
                        ),
                    );
                    return Ok(());
                }
            };

            let theme = match theme_param.as_association() {
                Ok(view) => {
                    let result: std::result::Result<HashMap<String, String>, _> = view
                        .iter()
                        .map(|r| r.map(|(k, v)| (k.to_string(), v.to_string())))
                        .collect();
                    match result {
                        Ok(t) => t,
                        Err(_) => {
                            let _ = env.set(
                                "_zsh_ts_error",
                                ParamSetValue::Scalar("_ZSH_TS_HIGHLIGHTER_THEME parsing failed"),
                            );
                            return Ok(());
                        }
                    }
                }
                Err(_) => {
                    let _ = env.set(
                        "_zsh_ts_error",
                        ParamSetValue::Scalar(
                            "_ZSH_TS_HIGHLIGHTER_THEME is not an associative array",
                        ),
                    );
                    return Ok(());
                }
            };

            if theme.is_empty() {
                let _ = env.set(
                    "_zsh_ts_error",
                    ParamSetValue::Scalar(
                        "_ZSH_TS_HIGHLIGHTER_THEME is empty (did you source activate.zsh?)",
                    ),
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
                    let _ = env.set(
                        "_zsh_ts_error",
                        ParamSetValue::Scalar(&format!("highlight failed: {}", err)),
                    );
                    return Ok(());
                }
            };

            // Validate commands and add override spans for invalid ones (zsh mode only).
            if language == LanguageConfig::Zsh {
                match engine.extract_zsh_commands(&full_source) {
                    Ok(commands) => {
                        for (start, end, name) in commands {
                            if !is_valid_command(&env, &name) {
                                spans.push(Span {
                                    start,
                                    end,
                                    style: "command.invalid".to_string(),
                                });
                            }
                        }
                    }
                    Err(err) => {
                        let _ = env.set(
                            "_zsh_ts_error",
                            ParamSetValue::Scalar(&format!("command extraction failed: {}", err)),
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
            if let Err(e) = env.set(
                "_zsh_ts_regions",
                ParamSetValue::Array(Box::new(regions.into_iter())),
            ) {
                let _ = env.set(
                    "_zsh_ts_error",
                    ParamSetValue::Scalar(&format!("failed to set regions: {:?}", e)),
                );
            }

            Ok(())
        }
    }
}

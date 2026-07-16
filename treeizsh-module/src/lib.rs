use oxizsh::zsh_module;

mod theme;

#[zsh_module]
mod treeizsh {
    use std::collections::HashMap;

    use treeizsh::highlight::{HighlightEngine, LanguageConfig, Span};
    use oxizsh::ParamSetValue;
    use oxizsh::{Association, Termination};

    // ---------------------------------------------------------------------------
    // Helper: validate commands using the high-level Zsh table APIs
    // ---------------------------------------------------------------------------

    fn is_path_executable(name: &str) -> bool {
        let path = std::path::Path::new(name);
        if !path.is_absolute() && !name.contains('/') {
            return false;
        }
        if let Ok(metadata) = std::fs::metadata(path) {
            if metadata.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    return metadata.permissions().mode() & 0o111 != 0;
                }
                #[cfg(not(unix))]
                {
                    return true;
                }
            }
        }
        false
    }

    // TODO: Handle commands in relative PATH entries (like '.' or './bin') that are not hashed by Zsh.
    // In Zsh, relative directory entries in $path are not pre-hashed in cmdnamtab (to avoid stale hashes on cd).
    // Currently, commands like `test_local` in the CWD (when `.` is in PATH) are marked as invalid because they do
    // not contain a `/` and are not in `env.commands()`.
    // In the future, we could resolve this by parsing the relative directories in $path against $PWD or falling
    // back to querying Zsh's built-in command type information (e.g. `type -w -- <cmd>`) similar to
    // fast-syntax-highlighting.
    fn is_valid_command(
        env: &oxizsh::Env,
        name: &str,
        local_functions: &std::collections::HashSet<String>,
    ) -> bool {
        env.commands().contains_key(name)
            || env.functions().contains_key(name)
            || env.builtins().contains_key(name)
            || env.aliases().contains_key(name)
            || local_functions.contains(name)
            || is_path_executable(name)
    }

    // ---------------------------------------------------------------------------
    // Module state & Component
    // ---------------------------------------------------------------------------

    #[zsh_component]
    #[derive(Default)]
    pub struct Highlighter {
        engine: Option<HighlightEngine>,
        theme_path: Option<String>,
        theme_cache: HashMap<String, String>,
    }

    impl std::fmt::Debug for Highlighter {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Highlighter")
                .field("theme_path", &self.theme_path)
                .field("theme_cache_len", &self.theme_cache.len())
                .finish()
        }
    }

    impl Highlighter {
        pub fn new() -> Self {
            Self::default()
        }

        // ---------------------------------------------------------------------------
        // Builtin: treeizsh_highlight
        // ---------------------------------------------------------------------------

        pub fn treeizsh_highlight(
            &mut self,
            env: &oxizsh::Env,
            _args: &[&str],
        ) -> Result<(), Termination> {

            // Lazy-initialize the highlight engine on first call.
            let engine = match self.engine.as_mut() {
                Some(e) => e,
                None => {
                    let e = match HighlightEngine::new() {
                        Ok(e) => e,
                        Err(err) => {
                            let _ = env.set(
                                "treeizsh_error",
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
                .get("TREEIZSH_MODE")
                .and_then(|p| p.as_scalar().map(|s| s.into_owned()))
                .ok()
                .unwrap_or_else(|| "zsh".into());
            let language = match mode.as_str() {
                "md" | "markdown" => LanguageConfig::Markdown,
                _ => LanguageConfig::Zsh,
            };

            // Read theme path.
            let theme_param = match env.get("TREEIZSH_THEME") {
                Ok(p) => p,
                Err(_) => {
                    let _ = env.set(
                        "treeizsh_error",
                        ParamSetValue::Scalar(
                            "TREEIZSH_THEME not set (did you source treeizsh.zsh?)",
                        ),
                    );
                    return Ok(());
                }
            };

            let theme_path_str = match theme_param.as_scalar() {
                Ok(s) => s.into_owned(),
                Err(_) => {
                    let _ = env.set(
                        "treeizsh_error",
                        ParamSetValue::Scalar("TREEIZSH_THEME is not a string"),
                    );
                    return Ok(());
                }
            };

            if theme_path_str.is_empty() {
                let _ = env.set(
                    "treeizsh_error",
                    ParamSetValue::Scalar("TREEIZSH_THEME is empty"),
                );
                return Ok(());
            }

            // Load and cache the theme if needed
            let needs_reload = match &self.theme_path {
                Some(cached_path) => cached_path != &theme_path_str || self.theme_cache.is_empty(),
                None => true,
            };

            if needs_reload {
                match crate::theme::load_theme_file(&theme_path_str) {
                    Ok(cache) => {
                        self.theme_cache = cache;
                        self.theme_path = Some(theme_path_str);
                    }
                    Err(err) => {
                        let _ = env.set(
                            "treeizsh_error",
                            ParamSetValue::Scalar(&format!("theme load failed: {}", err)),
                        );
                        return Ok(());
                    }
                }
            }

            let full_source = format!("{}{}", prebuffer, buffer);
            let prebuffer_char_len = prebuffer.chars().count();

            // Run tree-sitter highlighting (static + dynamic path resolution).
            let pwd_str = pwd.as_deref();
            let mut spans = match engine.highlight_with_pwd(language, &full_source, pwd_str) {
                Ok(s) => s,
                Err(err) => {
                    let _ = env.set(
                        "treeizsh_error",
                        ParamSetValue::Scalar(&format!("highlight failed: {}", err)),
                    );
                    return Ok(());
                }
            };

            // Validate commands and add override spans for invalid ones (zsh mode only).
            if language == LanguageConfig::Zsh {
                let local_functions = engine
                    .extract_zsh_function_definitions(&full_source)
                    .unwrap_or_default();

                match engine.extract_zsh_commands(&full_source) {
                    Ok(commands) => {
                        for (start, end, name) in commands {
                            if !is_valid_command(env, &name, &local_functions) {
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
                            "treeizsh_error",
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
                if let Some(style) = self.theme_cache.get(&span.style) {
                    regions.push(format!("{} {} {}", start, end, style));
                }
            }

            // Write the result array for the zsh adapter to pick up.
            if let Err(e) = env.set(
                "treeizsh_regions",
                ParamSetValue::Array(Box::new(regions.into_iter())),
            ) {
                let _ = env.set(
                    "treeizsh_error",
                    ParamSetValue::Scalar(&format!("failed to set regions: {:?}", e)),
                );
            }

            Ok(())
        }
    }
}

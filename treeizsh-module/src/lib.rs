use oxizsh::zsh_module;

mod theme;

#[zsh_module]
mod treeizsh {
    use std::collections::HashMap;

    use oxizsh_module::Termination;
    use treeizsh::highlight::{HighlightEngine, LanguageConfig, Span};

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
    fn is_in_assoc(env: &oxizsh::Env, base: &str, key: &str) -> bool {
        let value: Option<String> = env.get_index(base, key).unwrap_or(None);
        value.is_some()
    }

    fn is_valid_command(env: &oxizsh::Env, name: &str) -> bool {
        is_in_assoc(env, "commands", name)
            || is_in_assoc(env, "functions", name)
            || is_in_assoc(env, "builtins", name)
            || is_in_assoc(env, "aliases", name)
            || is_path_executable(name)
    }

    // ---------------------------------------------------------------------------
    // Module state & Component
    // ---------------------------------------------------------------------------

    /// Everything the highlight output depends on. When this matches the previous
    /// call, the previous regions are still correct (cursor moves, plain redraws).
    #[derive(PartialEq, Eq)]
    struct InputKey {
        source: String,
        prebuffer_len: usize,
        pwd: Option<String>,
        language: LanguageConfig,
        theme_path: String,
    }

    #[zsh_component]
    #[derive(Default)]
    pub struct Highlighter {
        engine: Option<HighlightEngine>,
        theme_path: Option<String>,
        theme_cache: HashMap<String, String>,
        /// Input of the last successful highlight, and the regions it produced.
        last_input: Option<InputKey>,
        last_regions: Vec<String>,
        /// Command name -> valid, for names not defined in the buffer itself.
        /// Cleared per command line and on directory change.
        command_cache: HashMap<String, bool>,
    }

    impl std::fmt::Debug for Highlighter {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Highlighter")
                .field("theme_path", &self.theme_path)
                .field("theme_cache_len", &self.theme_cache.len())
                .field("command_cache_len", &self.command_cache.len())
                .finish()
        }
    }

    impl Highlighter {
        pub fn new() -> Self {
            Self::default()
        }

        /// Forget everything cached about the outside world: command validity,
        /// path lookups, and the last result.
        fn reset(&mut self) {
            self.last_input = None;
            self.last_regions.clear();
            self.command_cache.clear();
            if let Some(engine) = self.engine.as_mut() {
                engine.clear_caches();
            }
        }

        // ---------------------------------------------------------------------------
        // Builtin: treeizsh_highlight
        // ---------------------------------------------------------------------------

        /// `treeizsh_highlight`: set `treeizsh_regions` for the current
        /// `PREBUFFER`/`BUFFER`, and `treeizsh_changed` to 0 when they are the
        /// regions from the previous call (nothing to redo), else 1.
        ///
        /// `treeizsh_highlight --reset`: drop all caches; call at the start of
        /// each command line so new commands and files are noticed.
        pub fn treeizsh_highlight(
            &mut self,
            env: &oxizsh::Env,
            args: &[&str],
        ) -> Result<(), Termination> {
            if args.first() == Some(&"--reset") {
                self.reset();
                return Ok(());
            }

            // Read ZLE and environment state.
            let buffer = env.get::<String>("BUFFER").unwrap_or_default();
            let prebuffer = env.get::<String>("PREBUFFER").unwrap_or_default();
            let pwd = env.get::<String>("PWD").ok();

            // Determine language mode.
            let mode = env
                .get::<String>("TREEIZSH_MODE")
                .unwrap_or_else(|_| "zsh".into());
            let language = match mode.as_str() {
                "md" | "markdown" => LanguageConfig::Markdown,
                _ => LanguageConfig::Zsh,
            };

            // Read theme path.
            let theme_path_str = match env.get::<String>("TREEIZSH_THEME") {
                Ok(s) => s,
                Err(e) => {
                    let msg = match e {
                        oxizsh::Error::NotFound(_) => {
                            "TREEIZSH_THEME not set (did you source treeizsh.zsh?)"
                        }
                        _ => "TREEIZSH_THEME is not a string",
                    };
                    let _ = env.set("treeizsh_error", msg);
                    return Ok(());
                }
            };

            if theme_path_str.is_empty() {
                let _ = env.set("treeizsh_error", "TREEIZSH_THEME is empty");
                return Ok(());
            }

            let input = InputKey {
                prebuffer_len: prebuffer.chars().count(),
                source: prebuffer + &buffer,
                pwd,
                language,
                theme_path: theme_path_str,
            };
            if self.last_input.as_ref() == Some(&input) {
                let _ = env.set("treeizsh_regions", self.last_regions.as_slice());
                let _ = env.set("treeizsh_changed", 0i64);
                return Ok(());
            }
            if self
                .last_input
                .as_ref()
                .is_some_and(|last| last.pwd != input.pwd)
            {
                // Relative command paths (./script) resolve differently now.
                self.command_cache.clear();
            }
            self.last_input = None;

            match self.compute_regions(env, &input) {
                Ok(regions) => {
                    if let Err(e) = env.set("treeizsh_regions", regions.as_slice()) {
                        let _ =
                            env.set("treeizsh_error", format!("failed to set regions: {:?}", e));
                        return Ok(());
                    }
                    let _ = env.set("treeizsh_changed", 1i64);
                    self.last_regions = regions;
                    self.last_input = Some(input);
                }
                Err(msg) => {
                    let _ = env.set("treeizsh_error", msg);
                }
            }
            Ok(())
        }

        /// Highlight `input` and turn the spans into `region_highlight` entries.
        fn compute_regions(
            &mut self,
            env: &oxizsh::Env,
            input: &InputKey,
        ) -> Result<Vec<String>, String> {
            // Lazy-initialize the highlight engine on first call.
            if self.engine.is_none() {
                let engine =
                    HighlightEngine::new().map_err(|err| format!("engine init failed: {}", err))?;
                self.engine = Some(engine);
            }
            let engine = self.engine.as_mut().unwrap();

            // Load and cache the theme if needed.
            if self.theme_path.as_ref() != Some(&input.theme_path) || self.theme_cache.is_empty() {
                self.theme_cache = crate::theme::load_theme_file(&input.theme_path)
                    .map_err(|err| format!("theme load failed: {}", err))?;
                self.theme_path = Some(input.theme_path.clone());
            }

            let mut spans = match input.language {
                LanguageConfig::Markdown => engine
                    .highlight(LanguageConfig::Markdown, &input.source)
                    .map_err(|err| format!("highlight failed: {}", err))?,
                LanguageConfig::Zsh => {
                    let analysis = engine
                        .analyze_zsh(&input.source, input.pwd.as_deref())
                        .map_err(|err| format!("highlight failed: {}", err))?;
                    let mut spans = analysis.spans;
                    // Mark commands that zsh can't run.
                    for command in analysis.commands {
                        if analysis.local_functions.contains(&command.name) {
                            continue;
                        }
                        let valid = match self.command_cache.get(&command.name) {
                            Some(&valid) => valid,
                            None => {
                                let valid = is_valid_command(env, &command.name);
                                self.command_cache.insert(command.name.clone(), valid);
                                valid
                            }
                        };
                        if !valid {
                            spans.push(Span {
                                start: command.start,
                                end: command.end,
                                style: "command.invalid".to_string(),
                            });
                        }
                    }
                    spans
                }
            };

            // Sort: by start position, and for equal starts longer spans first so that
            // "last wins" gives precedence to more specific spans.
            spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));

            // Shift coordinates from the full source to buffer-only, drop prebuffer
            // spans, and map capture names to theme attributes.
            let prebuffer_len = input.prebuffer_len;
            let mut regions = Vec::new();
            for span in spans {
                if span.end <= prebuffer_len {
                    continue;
                }
                let start = span.start.saturating_sub(prebuffer_len);
                let end = span.end - prebuffer_len;
                if start >= end {
                    continue;
                }
                if let Some(style) = self.theme_cache.get(&span.style) {
                    regions.push(format!("{} {} {}", start, end, style));
                }
            }
            Ok(regions)
        }
    }
}

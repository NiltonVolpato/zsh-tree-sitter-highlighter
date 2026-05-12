# zsh-tree-sitter-highlighter

A Zsh shell syntax highlighter daemon using tree-sitter for multi-language prompt highlighting.

## Features

- **Shell mode** (`tree-sitter-zsh`): zsh grammar with dynamic callable/path validation
- **Markdown mode** (`tree-sitter-md`): CommonMark/GFM highlighting via split block/inline parsers
- **Daemon architecture**: Unix socket protocol for low-latency highlighting on every keystroke
- **Hardcoded Tokyonight theme**: ANSI colors mapped to standard tree-sitter capture names

## Dependencies

- Rust toolchain (2024 edition)
- Zsh with `zsh/net/socket` module
- `tree-sitter`, `tree-sitter-highlight`, `tree-sitter-zsh`, `tree-sitter-md`

## Installation

```bash
cargo install --path .
```

## Setup

Add to your `.zshrc`:

```bash
eval "$(zsh-tree-sitter-highlighter activate)"
```

This starts the daemon (if not running) and registers a ZLE hook for `line-pre-redraw`.

## Environment Variables

- `FORGE_PROMPT_LANG`: Controls the language mode. Set to `zsh` (default) or `md`/`markdown` to switch prompt highlighting.

## CLI Commands

```bash
zsh-tree-sitter-highlighter activate   # Print Zsh integration script
zsh-tree-sitter-highlighter start      # Start the daemon
zsh-tree-sitter-highlighter stop       # Stop the daemon
zsh-tree-sitter-highlighter status     # Check daemon status
zsh-tree-sitter-highlighter highlight  # One-shot highlight (for testing)
```

## Inline Theme Capture Names

The hardcoded theme covers at minimum:

- `keyword`, `string`, `comment`, `function`, `variable`, `number`, `operator`, `constant`, `type`, `property`
- `punctuation.*`, `markup.heading`, `markup.strong`, `markup.italic`, `markup.link`, `markup.raw`, `markup.list`, `markup.list.checked`, `markup.list.unchecked`

Future versions will support external TOML theme files.

## Version 2 Roadmap

1. **`@[file]` dynamic highlighting in markdown mode**: Detect `@[...]` patterns, validate file existence, and apply underline/red spans.
2. **External theme files / theme conversion**: Add a TOML theme format and ideally an automatic converter from neovim Lua themes.

## License

MIT

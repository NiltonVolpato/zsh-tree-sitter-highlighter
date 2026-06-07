# zsh-tree-sitter-highlighter

A Zsh shell syntax highlighter using tree-sitter for prompt highlighting.

## Features

- **Shell mode** (`tree-sitter-zsh`): zsh grammar with dynamic command/path validation
- **Markdown mode** (`tree-sitter-md`): CommonMark/GFM highlighting via split block/inline parsers
- **Zsh module architecture**: Native zsh module written in Rust — no daemon, no sockets, no IPC
- **Incremental-friendly**: Parser state can be persisted across keystrokes (future optimization)
- **Tokyonight theme**: ANSI colors mapped to standard tree-sitter capture names

## Architecture

This project consists of:

- **`zsh-ts-highlighter/`**: Core Rust library with tree-sitter parsing and highlighting logic
- **`zsh-ts-module/`**: Zsh module (Rust `cdylib`) that integrates the highlighter directly into zsh

The module reads `BUFFER` and `PREBUFFER` directly via zsh's parameter API, performs tree-sitter highlighting, validates commands against zsh's internal hash tables, and writes `region_highlight` entries — all without leaving the zsh process.

## Dependencies

- Rust toolchain (2024 edition)
- Zsh with `add-zle-hook-widget` support (standard in modern zsh)
- `tree-sitter`, `tree-sitter-highlight`, `tree-sitter-zsh`, `tree-sitter-md`

## Build

```bash
cargo build -p zsh-ts-module
```

On macOS, the build produces `libzsh_ts_module.dylib`. The activation script automatically creates the `zsh_ts_module.bundle` symlink that zsh expects.

## Installation & Setup

For regular use, run the installer:

```bash
./install.sh
```

Follow the prompts to specify the installation directory. The script compiles the module in release mode and copies the necessary files.

After the installer finishes, add the following lines to your `~/.zshrc`:

```zsh
module_path+=("$HOME/.local/share/zsh-tree-sitter-highlighter")
zmodload zsh_ts_module
source "$HOME/.local/share/zsh-tree-sitter-highlighter/zsh-integration.zsh"
```

*(Adjust the path if you chose a custom installation directory)*

### Manual Setup (From Source)

If you prefer to load it directly from your build target directory:

```zsh
# Path to the directory containing libzsh_ts_module.*
module_path+=(/path/to/zsh-tree-sitter-highlighter/target/release)
zmodload zsh_ts_module

# Source the integration script (registers the ZLE hook)
source /path/to/zsh-tree-sitter-highlighter/zsh-ts-module/zsh-integration.zsh
```

## Theme Customization

The theme is an associative array mapping tree-sitter capture names to zsh `region_highlight` attributes:

| Capture Name | Default |
|-------------|---------|
| `comment` | `fg=#565f89` |
| `function` | `fg=#7aa2f7` |
| `string` | `fg=#e0af68` |
| `variable` | `fg=#bb9af7` |
| `command.invalid` | `fg=#f7768e` |
| `keyword` | `fg=#c099ff` |
| `number` | `fg=#ff9e64` |
| `operator` | `fg=#89ddff` |

Set `_ZSH_TS_HIGHLIGHTER_THEME` before loading the module to override defaults.

## Testing & Development

To compile the module and start an interactive testing shell:

```bash
./scripts/dev-build.zsh [--release|--debug]
```

To run the expect-based integration tests:

```bash
expect zsh-ts-module/test-activate.expect
expect zsh-ts-module/test-highlight.expect
```

## Legacy Daemon Mode

The original implementation used a Unix socket daemon with bencode IPC. This code still exists in `zsh-ts-highlighter/src/daemon.rs` but is no longer the recommended approach. The module architecture is simpler, faster, and requires no background process management.

## License


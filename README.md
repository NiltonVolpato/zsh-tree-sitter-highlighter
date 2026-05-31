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

## Setup

Add to your `.zshrc`:

```zsh
source /path/to/zsh-tree-sitter-highlighter/zsh-ts-module/activate.zsh
```

This single command:
1. Finds the compiled module (debug or release)
2. Creates the macOS `.bundle` symlink if needed
3. Loads the module via `zmodload`
4. Sets the default theme if you haven't configured one
5. Registers the `line-pre-redraw` ZLE hook

### Manual Setup (without activate.zsh)

If you prefer to set things up manually:

```zsh
# Path to the directory containing libzsh_ts_module.*
module_path+=(/path/to/zsh-tree-sitter-highlighter/target/debug)
zmodload zsh_ts_module

# Optional: customize the theme
typeset -gA _ZSH_TS_HIGHLIGHTER_THEME=(
    [comment]="fg=#565f89"
    [function]="fg=#7aa2f7"
    [string]="fg=#e0af68"
    [variable]="fg=#bb9af7"
    [command.invalid]="fg=#f7768e"
)

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

Set `_ZSH_TS_HIGHLIGHTER_THEME` **before** sourcing `activate.zsh` to override defaults.

## Testing

```bash
# Run the expect-based integration test
cd zsh-ts-module
expect test-activate.expect
```

Or test manually:

```bash
cargo build -p zsh-ts-module
source zsh-ts-module/activate.zsh
echo hello  # type this interactively; you should see syntax highlighting
```

## Legacy Daemon Mode

The original implementation used a Unix socket daemon with bencode IPC. This code still exists in `zsh-ts-highlighter/src/daemon.rs` but is no longer the recommended approach. The module architecture is simpler, faster, and requires no background process management.

## License


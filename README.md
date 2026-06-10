# treeizsh

A Zsh shell syntax highlighter using tree-sitter for prompt highlighting.

## Features

- **Shell mode** (`tree-sitter-zsh`): zsh grammar with dynamic command/path validation
- **Markdown mode** (`tree-sitter-md`): CommonMark/GFM highlighting via split block/inline parsers
- **Zsh module architecture**: Native zsh module written in Rust — no daemon, no sockets, no IPC
- **Incremental-friendly**: Parser state can be persisted across keystrokes (future optimization)
- **Tokyonight theme**: ANSI colors mapped to standard tree-sitter capture names

## Architecture

This project consists of:

- **`treeizsh/`**: Core Rust library with tree-sitter parsing and highlighting logic
- **`treeizsh-module/`**: Zsh module (Rust `cdylib`) that integrates the highlighter directly into zsh

The module reads `BUFFER` and `PREBUFFER` directly via zsh's parameter API, performs tree-sitter highlighting, validates commands against zsh's internal hash tables, and writes `region_highlight` entries — all without leaving the zsh process.

## Dependencies

- Rust toolchain (2024 edition)
- Zsh with `add-zle-hook-widget` support (standard in modern zsh)
- `tree-sitter`, `tree-sitter-highlight`, `tree-sitter-zsh`, `tree-sitter-md`

## Build

```bash
cargo build -p treeizsh-module
```

On macOS, the build produces `libtreeizsh.dylib`. The activation script automatically creates the `treeizsh.bundle` symlink that zsh expects.

## Installation & Setup

### Automated Installation (Recommended)

Run the manual installation script, which compiles the native module and copies all files to the target directory:

```bash
# It will ask interactively where to install.
# Default: ~/.local/share/treeizsh
./scripts/manual-install.sh
```

---

### Step-by-Step Manual Installation (From Source)

If you prefer to perform the installation steps manually:

1. Compile the native module library:
   ```bash
   cargo build --release -p treeizsh-module --lib
   ```

2. Copy the compiled artifacts, integration script, and themes to a directory of your choice `$DIR` (e.g., `~/.local/share/treeizsh`):
   ```bash
   # Set your installation directory
   DIR="$HOME/.local/share/treeizsh"
   mkdir -p "$DIR/$ZSH_VERSION"

   # On macOS:
   # Copy compiled library directly into the Zsh-versioned folder
   cp target/release/libtreeizsh.dylib "$DIR/$ZSH_VERSION/treeizsh.so"
   # Create the bundle symlink that Zsh expects
   ln -sf treeizsh.so "$DIR/$ZSH_VERSION/treeizsh.bundle"

   # On Linux:
   # Copy compiled library directly to the Zsh-versioned folder
   cp target/release/libtreeizsh.so "$DIR/$ZSH_VERSION/treeizsh.so"

   # Copy the Zsh integration script and themes directory
   cp treeizsh-module/treeizsh.zsh "$DIR/"
   cp -r treeizsh-module/themes "$DIR/"

   # Optional: clean the target folder to reclaim disk space (which can grow to gigabytes)
   cargo clean
   ```

3. Add the following line to your `~/.zshrc`:
   ```zsh
   source "$HOME/.local/share/treeizsh/treeizsh.zsh"
   ```

---

### Alternative Installation Methods (Coming Soon)

We plan to support automated installation scripts and package managers in future releases:

*   **Zsh Plugin Managers** (e.g., Oh-My-Zsh, Zinit, Antigen, ZPlug)
*   **Homebrew Formula** (`brew install treeizsh`)
*   **Standalone Installer Script** (`curl -fsSL https://treei.sh/install.sh | zsh`)

---

## Theme Customization

The theme is configured using a TOML file that maps tree-sitter capture names to standard Zsh `region_highlight` attributes. By default, the `onedark.toml` theme is used.

| Capture Name | Example Highlight Style |
|-------------|-------------------------|
| `comment` | `fg=#565f89` |
| `function` | `fg=#7aa2f7` |
| `string` | `fg=#e0af68` |
| `variable` | `fg=#bb9af7` |
| `command.invalid` | `fg=#f7768e` |
| `keyword` | `fg=#c099ff` |
| `number` | `fg=#ff9e64` |
| `operator` | `fg=#89ddff` |

To override the default theme, set the `TREEIZSH_THEME` environment variable in your `~/.zshrc` before sourcing the integration script:

```zsh
typeset -g TREEIZSH_THEME="/path/to/your/custom/theme.toml"
source "$HOME/.local/share/treeizsh/treeizsh.zsh"
```

---

## Testing & Development

### Local Interactive Development

To compile the module and launch a temporary, isolated subshell with the newly compiled module loaded for testing:

```bash
./scripts/dev-build.zsh [--release|--debug]
```

### Automated Integration and Unit Tests

All unit tests and ZLE integration tests are written in Rust using a virtual terminal environment.

To run the entire test suite:

```bash
cargo test
```

Unit tests cover AST parsing, theme color resolution, and highlighter correctness. ZLE integration tests verify syntax highlighting output on the terminal command line inside an active `zsh` session.

---

## Roadmap

- [x] Native Zsh module architecture
- [x] End-to-end ZLE testing framework in Rust
- [ ] Installer scripts and package managers
  - [ ] Standalone installer script
  - [ ] Homebrew tap/formula support
  - [ ] Pre-compiled release binaries
- [ ] Highlighter features
  - [ ] Incremental parsing optimization
  - [ ] Additional grammars (Python, TOML)

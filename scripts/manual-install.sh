#!/usr/bin/env zsh
set -eu

print -P "%F{cyan}%B=== Treeizsh Installer ===%b%f"
print -P "This script will compile the native Zsh module and set up the integration.\n"

: ${XDG_DATA_HOME:=~/.local/share}

local interactive=1
if [[ -n "${INSTALL_DIR:-}" ]]; then
    interactive=0
fi

# Default installation directory
if [[ -z "${INSTALL_DIR:-}" ]]; then
    INSTALL_DIR="$XDG_DATA_HOME/treeizsh"
    if ((interactive)) && [[ -t 0 ]]; then
        print -P "‣ %F{10}Enter installation directory (or press Enter; Ctrl-C to cancel):%f"
        vared -p "> " -e INSTALL_DIR
    fi
fi

WORKSPACE_DIR="${0:A:h:h}"

# Determine which zsh binary to compile against.
# The build system (oxizsh-build) uses $ZSH_BINARY if set, otherwise
# $SHELL (if it's zsh), then 'zsh' from PATH.
local zsh_bin
if [[ -n "${ZSH_BINARY:-}" ]]; then
    zsh_bin="$ZSH_BINARY"
elif [[ "$(basename "${SHELL:-}")" == "zsh" ]]; then
    zsh_bin="$SHELL"
else
    zsh_bin="zsh"
fi

# Resolve to an absolute path if it's not directly executable
if [[ ! -x "$zsh_bin" ]]; then
    zsh_bin="$(command -v "$zsh_bin" 2>/dev/null || echo "$zsh_bin")"
fi

local zsh_version="$($zsh_bin -f -c 'echo $ZSH_VERSION' 2>/dev/null)"
if [[ -z "$zsh_version" ]]; then
    print -u2 -P "%F{red}Error: unable to determine zsh version from '$zsh_bin'%f"
    exit 1
fi

if ((interactive)) && [[ -t 0 ]]; then
    print -P "‣ %F{10}Zsh binary to compile against (detected version $zsh_version):%f"
    vared -p "> " -e zsh_bin
    # Re-resolve and re-check after user edit
    if [[ ! -x "$zsh_bin" ]]; then
        zsh_bin="$(command -v "$zsh_bin" 2>/dev/null || echo "$zsh_bin")"
    fi
    zsh_version="$($zsh_bin -f -c 'echo $ZSH_VERSION' 2>/dev/null)"
    if [[ -z "$zsh_version" ]]; then
        print -u2 -P "%F{red}Error: unable to determine zsh version from '$zsh_bin'%f"
        exit 1
    fi
fi

print -P "%B‣ Target zsh: %F{cyan}${zsh_bin}%f (version %F{cyan}${zsh_version}%f)%b"

# Pass ZSH_BINARY to cargo build so oxizsh-build uses the same binary.
export ZSH_BINARY="$zsh_bin"

# Read cargo config from env
local skip_build="${SKIP_BUILD:-0}"
local cargo_build_dir="${CARGO_BUILD_DIR:-target/release}"

if ((!skip_build)); then
    print -P "%B‣ Building module in release mode...%b"
    if ! cargo build --manifest-path "$WORKSPACE_DIR/Cargo.toml" -p treeizsh-module --lib --release; then
        print -u2 -P "%F{red}Error: cargo build failed%f"
        exit 1
    fi
fi

print -P "%B‣ Setting up installation directory at: %F{cyan}${INSTALL_DIR}%f%b"
mkdir -p "$INSTALL_DIR/$zsh_version"

# Determine OS type and copy dynamic library using cp -l with fallback
rm -f "$INSTALL_DIR/$zsh_version/treeizsh.so" "$INSTALL_DIR/$zsh_version/treeizsh.bundle"
if [[ "$OSTYPE" == "darwin"* ]]; then
    print -P "‣ Installing for macOS..."
    cp -l "$WORKSPACE_DIR/${cargo_build_dir}/libtreeizsh.dylib" "$INSTALL_DIR/$zsh_version/treeizsh.so" 2>/dev/null ||
        cp "$WORKSPACE_DIR/${cargo_build_dir}/libtreeizsh.dylib" "$INSTALL_DIR/$zsh_version/treeizsh.so"
    ln -sf treeizsh.so "$INSTALL_DIR/$zsh_version/treeizsh.bundle"
else
    print -P "‣ Installing for Linux/Unix..."
    cp -l "$WORKSPACE_DIR/${cargo_build_dir}/libtreeizsh.so" "$INSTALL_DIR/$zsh_version/treeizsh.so" 2>/dev/null ||
        cp "$WORKSPACE_DIR/${cargo_build_dir}/libtreeizsh.so" "$INSTALL_DIR/$zsh_version/treeizsh.so"
fi

print -P "‣ Copying integration script and themes..."
cp "$WORKSPACE_DIR/treeizsh-module/treeizsh.zsh" "$INSTALL_DIR/"
cp -r "$WORKSPACE_DIR/treeizsh-module/themes" "$INSTALL_DIR/"

print -P "\n%F{green}%B✔ Installation completed successfully!%b%f"

# Determine if we should modify ~/.zshrc
local modify_zshrc="${MODIFY_ZSHRC:-}"
if [[ -z "$modify_zshrc" ]]; then
    if ((interactive)) && [[ -t 0 ]]; then
        print -n -P "‣ %F{10}Would you like to automatically modify your ~/.zshrc to source the plugin? [Y/n]:%f "
        read -r modify_zshrc
        if [[ -z "$modify_zshrc" ]]; then
            modify_zshrc="y"
        fi
    else
        modify_zshrc="n"
    fi
fi

if [[ "$modify_zshrc" =~ ^([Yy]|[Yy][Ee][Ss]|1)$ ]]; then
    local zshrc="${ZDOTDIR:-$HOME}/.zshrc"
    local line="source \"$INSTALL_DIR/treeizsh.zsh\""

    if [[ -f "$zshrc" ]] && grep -Fxq "$line" "$zshrc"; then
        print -P "%F{yellow}⚠ Plugin activation line is already present in $zshrc.%f"
    else
        echo "" >>"$zshrc"
        echo "# Treeizsh Highlighter" >>"$zshrc"
        echo "$line" >>"$zshrc"
        print -P "%F{green}✔ Appended plugin activation to $zshrc%f"
    fi
    print -P "Restart your terminal or run %F{cyan}exec zsh%f to activate the highlighter!"
else
    print -P "To activate the highlighter manually, add the following line to your %B~/.zshrc%b:"
    print -P "%F{cyan}source \"$INSTALL_DIR/treeizsh.zsh\"%f"
fi

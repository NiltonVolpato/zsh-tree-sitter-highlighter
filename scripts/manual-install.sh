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
mkdir -p "$INSTALL_DIR/$ZSH_VERSION"

# Determine OS type and copy dynamic library using cp -l with fallback
if [[ "$OSTYPE" == "darwin"* ]]; then
    print -P "‣ Installing for macOS..."
    cp -l "$WORKSPACE_DIR/${cargo_build_dir}/libtreeizsh.dylib" "$INSTALL_DIR/$ZSH_VERSION/treeizsh.so" 2>/dev/null ||
        cp "$WORKSPACE_DIR/${cargo_build_dir}/libtreeizsh.dylib" "$INSTALL_DIR/$ZSH_VERSION/treeizsh.so"
    ln -sf treeizsh.so "$INSTALL_DIR/$ZSH_VERSION/treeizsh.bundle"
else
    print -P "‣ Installing for Linux/Unix..."
    cp -l "$WORKSPACE_DIR/${cargo_build_dir}/libtreeizsh.so" "$INSTALL_DIR/$ZSH_VERSION/treeizsh.so" 2>/dev/null ||
        cp "$WORKSPACE_DIR/${cargo_build_dir}/libtreeizsh.so" "$INSTALL_DIR/$ZSH_VERSION/treeizsh.so"
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

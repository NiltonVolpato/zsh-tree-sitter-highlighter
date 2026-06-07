#!/usr/bin/env zsh
# zsh-tree-sitter-highlighter developer module activation script
#
# Usage: ./scripts/dev-build.zsh [--release|--debug]
#
# This script is for local development only. It builds the Rust module, and
# starts another zsh shell with the new module loaded for interactive testing.

WORKSPACE_DIR="${0:A:h:h}"

() {
    emulate -L zsh

    print -P "%B‣ Building module...%b"
    if ! cargo build --manifest-path "$WORKSPACE_DIR/Cargo.toml" -p zsh-ts-module --lib  "$@"; then
        print -u2 "zsh-tree-sitter-highlighter: cargo build failed"
        return 1
    fi
    print

    if (( ${@[(Ie)--release]} )); then
        local module_dir="$WORKSPACE_DIR/target/release"
    else
        local module_dir="$WORKSPACE_DIR/target/debug"
    fi

    local temp_dir="$(mktemp -d)"
    print -P "%F{8}‣ Creating temporary zsh dotfile directory: ${temp_dir}%f"
    print
    cat <<EOF >"${temp_dir}/.zshrc"
typeset -g -aU module_path
module_path+=("$module_dir")

# Load the module
if ! zmodload zsh_ts_module 2>/dev/null; then
    print -u2 "zsh-tree-sitter-highlighter: failed to load zsh_ts_module from ${module_dir}"
    return 1
fi

source "${WORKSPACE_DIR}/zsh-ts-module/zsh-integration.zsh"
EOF

    print -P "%B‣ Starting a subshell. Run 'exit' (Ctrl-D) to return.%b"
    print
    ZDOTDIR="${temp_dir}" zsh -i
    rm "${temp_dir}/.zshrc"
    rmdir "${temp_dir}"
} "$@" || return $?

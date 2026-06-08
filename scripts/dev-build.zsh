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

    local flag="--debug"
    if (( ${@[(Ie)--release]} )); then
        flag="--release"
    fi

    local temp_dir="$(mktemp -d)"
    print -P "%F{8}‣ Creating temporary zsh dotfile directory: ${temp_dir}%f"
    print
    cat <<EOF >"${temp_dir}/.zshrc"
source "${WORKSPACE_DIR}/zsh-ts-module/zsh-integration.zsh" ${flag}
EOF

    print -P "%B‣ Starting a subshell. Run 'exit' (Ctrl-D) to return.%b"
    print
    ZDOTDIR="${temp_dir}" zsh -i
    rm -rf "${temp_dir}/"
} "$@" || return $?

#!/usr/bin/env zsh
set -eu
setopt pipefail

# Commands to compare
if (($# < 1)); then
    print -P "%F{red}Usage: $0 <command_to_highlight>%f"
    exit 1
fi
CMD="$1"

# Locate the repository root
SCRIPT_DIR="${0:A:h}"
REPO_DIR="${SCRIPT_DIR:h}"

# Build/Rebuild native module if missing or stale
OS="$(uname -s)"
if [[ "$OS" == "Darwin" ]]; then
    LIB_NAME="libtreeizsh.dylib"
else
    LIB_NAME="libtreeizsh.so"
fi
LIB_PATH="$REPO_DIR/target/release/$LIB_NAME"

rebuild_module() {
    print -P "%F{yellow}%BBuilding/rebuilding native module for treeizsh...%b%f"
    local active_zsh
    active_zsh="$(which zsh)"

    ZSH_BINARY="$active_zsh" cargo build --manifest-path "$REPO_DIR/Cargo.toml" -p treeizsh-module --lib --release

    # Sleep a bit to allow OS validation and filesystem sync of the new .dylib/so
    sleep 1
}

if [[ ! -f "$LIB_PATH" ]]; then
    rebuild_module
elif command -v fd >/dev/null && command -v gstat >/dev/null; then
    STALE_FILES="$(fd --changed-after "@$(gstat -c %Y "$LIB_PATH")" . "$REPO_DIR")"
    if [[ -n "$STALE_FILES" ]]; then
        print -P "%F{yellow}%BNative module is stale. Modified files:%b%f"
        echo "$STALE_FILES"
        rebuild_module
    fi
fi

# Paths to plugins
TREEIZSH_PATH="$REPO_DIR/treeizsh-module/treeizsh.zsh"
FAST_PATH="/opt/homebrew/opt/zsh-fast-syntax-highlighting/share/zsh-fast-syntax-highlighting/fast-syntax-highlighting.plugin.zsh"

# Temporary files for expect scripts
EXPECT_TREEIZSH="$(mktemp /tmp/treeizsh_compare_XXXXXX.expect)"
EXPECT_FAST="$(mktemp /tmp/fast_compare_XXXXXX.expect)"

# Clean up temporary files on exit
cleanup() {
    rm -f "$EXPECT_TREEIZSH" "$EXPECT_FAST"
}
trap cleanup EXIT

# 1. Generate expect script for treeizsh
cat <<'EOF' >"$EXPECT_TREEIZSH"
log_user 1
set timeout 5
set cmd [lindex $argv 0]
set path [lindex $argv 1]
set repo_dir [lindex $argv 2]

spawn zsh -f
expect "% "
send "zmodload zsh/parameter; zmodload zsh/terminfo; zmodload zsh/termcap\r"
expect "% "
send "TREEIZSH_MODULE_PATH=\"$repo_dir/target/release\" source $path\r"
expect "% "
send "abort_command() { zle -I; BUFFER=\"\" }; zle -N abort_command; bindkey \"^G\" abort_command\r"
expect "% "
send "PROMPT=\"%# \"\r"
expect "% "

send -- "$cmd"
sleep 0.1
send "\u000c\u0007"
expect "% "

send "exit\r"
expect eof
EOF

# 2. Generate expect script for fast-syntax-highlighting
cat <<'EOF' >"$EXPECT_FAST"
log_user 1
set timeout 5
set cmd [lindex $argv 0]
set path [lindex $argv 1]

spawn zsh -f
expect "% "
send "zmodload zsh/parameter; zmodload zsh/terminfo; zmodload zsh/termcap\r"
expect "% "
send "source $path\r"
expect "% "
send "abort_command() { zle -I; BUFFER=\"\" }; zle -N abort_command; bindkey \"^G\" abort_command\r"
expect "% "
send "PROMPT=\"%# \"\r"
expect "% "

send -- "$cmd"
sleep 0.1
send "\u000c\u0007"
expect "% "

send "exit\r"
expect eof
EOF

print -P "%F{cyan}%B=== Command to Highlight ===%b%f"
echo
echo "$CMD"
echo

# Run treeizsh
print -P "%F{green}%B=== Treeizsh (Tree-sitter) ===%b%f"
echo
if [[ -f "$TREEIZSH_PATH" ]]; then
    # Run and print colorized output directly to terminal
    env TERM=xterm-256color expect "$EXPECT_TREEIZSH" "$CMD" "$TREEIZSH_PATH" "$REPO_DIR" | gsed -z 's/%[^%]*$//' | gsed -z 's/.*% //'

    # Also print BBcode representation
    env TERM=xterm-256color expect "$EXPECT_TREEIZSH" "$CMD" "$TREEIZSH_PATH" "$REPO_DIR" | gsed -z 's/%[^%]*$//' | gsed -z 's/.*% //' | ansifilter -B
else
    print -P "%F{red}  [Error: treeizsh integration script not found at $TREEIZSH_PATH]%f"
fi
echo

# Run fast-syntax-highlighting
print -P "%F{yellow}%B=== Fast-Syntax-Highlighting ===%b%f"
echo
if [[ -f "$FAST_PATH" ]]; then
    # Run and print colorized output directly to terminal
    env TERM=xterm-256color expect "$EXPECT_FAST" "$CMD" "$FAST_PATH" | gsed -z 's/%[^%]*$//' | gsed -z 's/.*% //'

    # Also print BBcode representation
    env TERM=xterm-256color expect "$EXPECT_FAST" "$CMD" "$FAST_PATH" | gsed -z 's/%[^%]*$//' | gsed -z 's/.*% //' | ansifilter -B
else
    print -P "%F{yellow}  [Warning: fast-syntax-highlighting plugin not found at $FAST_PATH]%f"
fi
echo

#!/usr/bin/env bash
set -euo pipefail

# Commands to compare
if [[ $# -lt 1 ]]; then
    echo "Usage: $0 <command_to_highlight>"
    exit 1
fi
CMD="$1"

# Paths to plugins
TREEIZSH_PATH="/Users/nilton/.local/share/treeizsh/treeizsh.zsh"
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

echo -e "\033[1;36m=== Command to Highlight ===\033[0m"
echo
echo "$CMD"
echo

# Run treeizsh
echo -e "\033[1;32m=== Treeizsh (Tree-sitter) ===\033[0m"
echo
if [[ -f "$TREEIZSH_PATH" ]]; then
    # Run and print colorized output directly to terminal
    env TERM=xterm-256color expect "$EXPECT_TREEIZSH" "$CMD" "$TREEIZSH_PATH" | gsed -z 's/%[^%]*$//' | gsed -z 's/.*% //'

    # Also print BBcode representation
    env TERM=xterm-256color expect "$EXPECT_TREEIZSH" "$CMD" "$TREEIZSH_PATH" | gsed -z 's/%[^%]*$//' | gsed -z 's/.*% //' | ansifilter -B
else
    echo "  [Error: treeizsh integration script not found at $TREEIZSH_PATH]"
fi
echo

# Run fast-syntax-highlighting
echo -e "\033[1;33m=== Fast-Syntax-Highlighting ===\033[0m"
echo
if [[ -f "$FAST_PATH" ]]; then
    # Run and print colorized output directly to terminal
    env TERM=xterm-256color expect "$EXPECT_FAST" "$CMD" "$FAST_PATH" | gsed -z 's/%[^%]*$//' | gsed -z 's/.*% //'

    # Also print BBcode representation
    env TERM=xterm-256color expect "$EXPECT_FAST" "$CMD" "$FAST_PATH" | gsed -z 's/%[^%]*$//' | gsed -z 's/.*% //' | ansifilter -B
else
    echo "  [Warning: fast-syntax-highlighting plugin not found at $FAST_PATH]"
fi
echo

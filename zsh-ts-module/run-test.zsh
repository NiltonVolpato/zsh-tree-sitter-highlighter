#!/usr/bin/env zsh
set -e

MODULE_DIR="$(cd "$(dirname "$0")/../target/debug" && pwd)"
cd "$MODULE_DIR"

# macOS needs .bundle symlink without lib prefix
if [[ ! -f zsh_ts_module.bundle && -f libzsh_ts_module.dylib ]]; then
    ln -sf libzsh_ts_module.dylib zsh_ts_module.bundle
fi

zsh -df -c "
    module_path+=(\$PWD)
    zmodload zsh_ts_module

    typeset -gA _ZSH_TS_HIGHLIGHTER_THEME=(
        [comment]=fg=#565f89
        [string]=fg=#e0af68
        [function]=fg=#7aa2f7
        [command.invalid]=fg=#f7768e,bold
        [variable]=fg=#e0af68
        [operator]=fg=#89ddff
    )

    echo '=== Test 1: simple command ==='
    BUFFER='echo hello # comment'
    PREBUFFER=''
    zsh_ts_highlight
    for r in \"\${_zsh_ts_regions[@]}\"; do echo \"  \$r\"; done

    echo
    echo '=== Test 2: invalid command ==='
    BUFFER='nonexistent_cmd_12345'
    PREBUFFER=''
    zsh_ts_highlight
    for r in \"\${_zsh_ts_regions[@]}\"; do echo \"  \$r\"; done

    echo
    echo '=== Test 3: variable ==='
    BUFFER='echo \$HOME'
    PREBUFFER=''
    zsh_ts_highlight
    for r in \"\${_zsh_ts_regions[@]}\"; do echo \"  \$r\"; done

    echo
    echo '=== Test 4: prebuffer offset ==='
    BUFFER='echo world'
    PREBUFFER='echo hello'
    # Add real newline to prebuffer
    PREBUFFER=\$PREBUFFER\$'\n'
    zsh_ts_highlight
    for r in \"\${_zsh_ts_regions[@]}\"; do echo \"  \$r\"; done

    echo
    echo 'SUCCESS'
"

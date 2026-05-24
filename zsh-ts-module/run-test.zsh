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
    zsh_ts_highlight
    echo 'SUCCESS'
"

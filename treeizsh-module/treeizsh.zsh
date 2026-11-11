# treeizsh module integration script

function _treeizsh_highlighter() {
    # Keep module-internal variables local so they don't leak to global scope
    local -a treeizsh_regions
    local treeizsh_error treeizsh_changed

    # Call the Rust module to populate treeizsh_regions
    treeizsh_highlight

    # Report any error message from the module
    if [[ -n "${treeizsh_error}" ]]; then
        { zle -M "treeizsh: ${treeizsh_error}"; } >/dev/null 2>/dev/null
    fi

    # Same regions as last time (e.g. the cursor moved): leave region_highlight
    # alone, unless something else wiped our entries out of it.
    if [[ $treeizsh_changed == 0 ]] && {
        # (I) searches from the end, where our entries sit, so this is O(1).
        (( ! $#treeizsh_regions || ${region_highlight[(I)*memo=treeizsh]} ))
    }; then
        return
    fi

    # Replace our old highlights (tagged with a memo) with the new ones
    region_highlight=(
        ${region_highlight:#*memo=treeizsh}
        "${^treeizsh_regions[@]} memo=treeizsh"
    )
}

# Forget cached command/path lookups at the start of every command line, so
# newly installed commands, defined functions and created files are noticed.
function _treeizsh_line_init() {
    treeizsh_highlight --reset
}

autoload -U add-zle-hook-widget
add-zle-hook-widget line-init _treeizsh_line_init
add-zle-hook-widget line-pre-redraw _treeizsh_highlighter

local script_dir="${0:A:h}"

typeset -gU MODULE_PATH module_path
module_path+=("${TREEIZSH_MODULE_PATH:-$script_dir/$ZSH_VERSION}")

zmodload treeizsh
if (($?)); then
    print -P -u2 "%F{red}%Btreeizsh: Compiled module not found for Zsh $ZSH_VERSION.%f%b"
    return 1
fi

# Set default theme if user hasn't configured one
if ((!${+TREEIZSH_THEME})); then
    typeset -g TREEIZSH_THEME="${script_dir}/themes/onedark.toml"
fi

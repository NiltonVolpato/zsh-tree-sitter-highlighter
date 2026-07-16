# treeizsh module integration script

function _treeizsh_highlighter() {
    # Remove old highlights produced by this highlighter
    region_highlight=(${region_highlight:#*memo=treeizsh})

    # Keep module-internal variables local so they don't leak to global scope
    local -a treeizsh_regions
    local treeizsh_error

    # Call the Rust module to populate treeizsh_regions
    treeizsh_highlight

    # Report any error message from the module
    if [[ -n "${treeizsh_error}" ]]; then
        { zle -M "treeizsh: ${treeizsh_error}"; } >/dev/null 2>/dev/null
    fi

    # Append new highlights with memo tag so they can be filtered next time
    local r
    for r in "${treeizsh_regions[@]}"; do
        region_highlight+=("${r} memo=treeizsh")
    done
}
autoload -U add-zle-hook-widget
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

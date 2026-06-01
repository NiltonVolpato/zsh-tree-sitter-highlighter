# zsh-tree-sitter-highlighter module integration script
#
# Load this in your .zshrc after building the module:
#   module_path+=(/path/to/zsh-ts-module/target/debug)
#   zmodload zsh_ts_module
#   source /path/to/zsh-ts-module/zsh-integration.zsh

function _zsh_ts_highlighter() {
    # Remove old highlights produced by this highlighter
    region_highlight=( ${region_highlight:#*memo=zsh_ts_highlighter} )

    # Keep module-internal variables local so they don't leak to global scope
    local -a _zsh_ts_regions
    local _zsh_ts_error

    # Call the Rust module to populate _zsh_ts_regions
    zsh_ts_highlight

    # Report any error message from the module
    if [[ -n "${_zsh_ts_error}" ]]; then
        { zle -M "zsh-ts-highlighter: ${_zsh_ts_error}" } >/dev/null 2>/dev/null
    fi

    # Append new highlights with memo tag so they can be filtered next time
    local r
    for r in "${_zsh_ts_regions[@]}"; do
        region_highlight+=( "${r} memo=zsh_ts_highlighter" )
    done
}

autoload -U add-zle-hook-widget
add-zle-hook-widget line-pre-redraw _zsh_ts_highlighter

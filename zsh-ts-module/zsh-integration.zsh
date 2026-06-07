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

# Set default theme if user hasn't configured one
if (( ! ${+_ZSH_TS_HIGHLIGHTER_THEME} )); then
    typeset -gA _ZSH_TS_HIGHLIGHTER_THEME=(
        [comment]="fg=#565f89"
        [constant]="fg=#ff5370"
        [embedded]="fg=#73daca"
        [function]="fg=#7aa2f7"
        [keyword]="fg=#c099ff"
        [number]="fg=#ff9e64"
        [operator]="fg=#89ddff"
        [property]="fg=#73daca"
        [string]="fg=#e0af68"
        [text.emphasis]="fg=#c099ff"
        [text.literal]="fg=#e0af68"
        [text.reference]="fg=#7aa2f7"
        [text.strong]="fg=#c099ff"
        [text.title]="fg=#7aa2f7"
        [text.uri]="fg=#73daca"
        [punctuation.delimiter]="fg=#89ddff"
        [punctuation.special]="fg=#89ddff"
        [string.escape]="fg=#ff5370"
        [none]=""
        [path]="underline"
        [path.directory]="fg=#7aa2f7,underline"
        [command.invalid]="fg=#f7768e"
        [variable]="fg=#bb9af7"
    )
fi


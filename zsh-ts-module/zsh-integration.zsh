# zsh-tree-sitter-highlighter module integration script

local script_dir="${0:A:h}"
local release_mode=0
local debug_mode=0

# Parse options passed when sourcing the script
while (( $# > 0 )); do
    case "$1" in
        --release) release_mode=1 ;;
        --debug) debug_mode=1 ;;
    esac
    shift
done

# Load the compiled binary if not already loaded
if ! zmodload -F zsh_ts_module &>/dev/null; then
    local loaded=0
    local module_dir

    if (( release_mode )); then
        # Developer fallback: check target/release
        module_dir="$script_dir/../target/release"
        module_path+=("$module_dir")
        zmodload zsh_ts_module && loaded=1
    elif (( debug_mode )); then
        # Developer fallback: check target/debug
        module_dir="$script_dir/../target/debug"
        module_path+=("$module_dir")
        zmodload zsh_ts_module && loaded=1
    else
        # Production distribution layout: check dist/$ZSH_VERSION
        if [[ -f "$script_dir/$ZSH_VERSION/zsh_ts_module.so" || -f "$script_dir/$ZSH_VERSION/zsh_ts_module.bundle" ]]; then
            module_dir="$script_dir/$ZSH_VERSION"
            module_path+=("$module_dir")
            zmodload zsh_ts_module && loaded=1
        fi
    fi

    if (( ! loaded )); then
        print -P -u2 "%F{red}%Bzsh-tree-sitter-highlighter: Compiled module not found for Zsh $ZSH_VERSION.%f%b"
        if (( release_mode || debug_mode )); then
            print -P -u2 "Please build the module using: %F{cyan}cargo build --release -p zsh-ts-module --lib%f"
        else
            print -P -u2 "Please build and install the module inside: $script_dir"
        fi
        return 1
    fi
fi

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
    typeset -g _ZSH_TS_HIGHLIGHTER_THEME="${script_dir}/themes/onedark.toml"
fi


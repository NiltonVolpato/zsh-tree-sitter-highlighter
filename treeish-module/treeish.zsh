# treeish module integration script

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
if ! zmodload -F treeish &>/dev/null; then
    local loaded=0
    local module_dir

    if (( release_mode )); then
        # Developer fallback: check target/release
        module_dir="$script_dir/../target/release"
        module_path+=("$module_dir")
        zmodload treeish && loaded=1
    elif (( debug_mode )); then
        # Developer fallback: check target/debug
        module_dir="$script_dir/../target/debug"
        module_path+=("$module_dir")
        zmodload treeish && loaded=1
    else
        # Production distribution layout: check dist/$ZSH_VERSION
        if [[ -f "$script_dir/$ZSH_VERSION/treeish.so" || -f "$script_dir/$ZSH_VERSION/treeish.bundle" ]]; then
            module_dir="$script_dir/$ZSH_VERSION"
            module_path+=("$module_dir")
            zmodload treeish && loaded=1
        fi
    fi

    if (( ! loaded )); then
        print -P -u2 "%F{red}%Btreeish: Compiled module not found for Zsh $ZSH_VERSION.%f%b"
        if (( release_mode || debug_mode )); then
            print -P -u2 "Please build the module using: %F{cyan}cargo build --release -p treeish-module --lib%f"
        else
            print -P -u2 "Please build and install the module inside: $script_dir"
        fi
        return 1
    fi
fi

function _treeish_highlighter() {
    # Remove old highlights produced by this highlighter
    region_highlight=( ${region_highlight:#*memo=treeish} )

    # Keep module-internal variables local so they don't leak to global scope
    local -a treeish_regions
    local treeish_error

    # Call the Rust module to populate treeish_regions
    treeish_highlight

    # Report any error message from the module
    if [[ -n "${treeish_error}" ]]; then
        { zle -M "treeish: ${treeish_error}" } >/dev/null 2>/dev/null
    fi

    # Append new highlights with memo tag so they can be filtered next time
    local r
    for r in "${treeish_regions[@]}"; do
        region_highlight+=( "${r} memo=treeish" )
    done
}

autoload -U add-zle-hook-widget
add-zle-hook-widget line-pre-redraw _treeish_highlighter

# Set default theme if user hasn't configured one
if (( ! ${+TREEISH_THEME} )); then
    typeset -g TREEISH_THEME="${script_dir}/themes/onedark.toml"
fi

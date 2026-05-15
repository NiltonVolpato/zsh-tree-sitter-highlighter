# WARNING: Do not cache or source this file manually. Its contents are generated
# automatically when zsh-tree-sitter-highlighter is started via the `activate`
# command. To set up, add the following to your .zshrc:
#
#   eval "$(zsh-tree-sitter-highlighter activate)"

zsh-tree-sitter-highlighter() {
    "$_ZSH_TS_HIGHLIGHTER_PATH" "$@"
}

# Print a key=length:value record for the protocol.
# Usage: print_kv key "$value"
print_kv() {
    print -r -- "$1=${#2}:${2}"
}

_zsh_ts_highlighter() {
    # remove tokens we have set earlier
    region_highlight=( ${region_highlight:#*memo=zsh_ts_highlighter} )

    # return immediately if buffer is empty
    [[ -z "$BUFFER" ]] && return

    local socket_path="$_ZSH_TS_HIGHLIGHTER_RUNTIME_DIR/daemon.sock"
    if [[ ! -S "$socket_path" ]]; then
        zle -M "zsh-tree-sitter-highlighter: daemon socket not found at $socket_path"
        return
    fi

    if ! zsocket "$socket_path" 2>/dev/null; then
        zle -M "zsh-tree-sitter-highlighter: failed to connect to socket at $socket_path."
        return
    fi
    local fd=$REPLY

    local lang="${FORGE_PROMPT_LANG:-zsh}"

    if [[ -z "$_ZSH_TS_HIGHLIGHTER_VERSION" ]]; then
        zle -M "zsh-tree-sitter-highlighter: _ZSH_TS_HIGHLIGHTER_VERSION not set, activation may have failed"
        exec {fd}>&-
        return
    fi

    {
        print_kv ver "$_ZSH_TS_HIGHLIGHTER_VERSION"
        print_kv lang "$lang"
        print_kv pwd "$PWD"
        print_kv prebuffer "$PREBUFFER"
        print_kv buffer "$BUFFER"
        print_kv EOM ""
    } >&$fd || {
        exec {fd}>&-
        zle -M "zsh-tree-sitter-highlighter: failed to send request to daemon"
        return
    }

    local -a new_regions=("${region_highlight[@]}")
    local line
    while IFS= read -r -u $fd line; do
        [[ -z "$line" ]] && continue
        new_regions+=("$line memo=zsh_ts_highlighter")
    done

    exec {fd}>&-

    region_highlight=("${new_regions[@]}")

    if [[ -z "${new_regions[@]}" ]]; then
        zle -M "zsh-tree-sitter-highlighter: daemon returned no highlights (version mismatch?)"
    fi
}

if ! zmodload zsh/net/socket 2>/dev/null; then
    print -u2 "zsh-tree-sitter-highlighter: failed to load zsh/net/socket module"
fi

autoload -U add-zle-hook-widget
add-zle-hook-widget line-pre-redraw _zsh_ts_highlighter

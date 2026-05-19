# WARNING: Do not cache or source this file manually. Its contents are generated
# automatically when zsh-tree-sitter-highlighter is started via the `activate`
# command. To set up, add the following to your .zshrc:
#
#   eval "$(zsh-tree-sitter-highlighter activate)"

zsh-tree-sitter-highlighter() {
    "$_ZSH_TS_HIGHLIGHTER_PATH" "$@"
}

# Encodes an associative array into a bencode string.
bencode_encode() {
    emulate -L zsh
    unsetopt multibyte
    print -nr "d"
    for k v in "${(kv)BENCODE_MSG[@]}"; do
        print -nr "${#k}:${k}"
        print -nr "${#v}:${v}"
    done
    print -nr "e"
    return 0
}

# Decodes a bencode string into an associative array.
bencode_decode() {
    emulate -L zsh
    unsetopt multibyte
    local string="$1"
    [[ -z $string || ${string:0:1} != "d" || ${string: -1} != "e" ]] && return 1
    string="${string:1:-1}"
    while [[ -n $string ]]; do
        local len="${string%%:*}"
        local offset=$(( ${#len} + 1 ))
        local key="${string:$offset:$len}"
        string="${string:$offset + $len}"

        local len="${string%%:*}"
        local offset=$(( ${#len} + 1 ))
        local value="${string:$offset:$len}"
        string="${string:$offset + $len}"

        BENCODE_MSG[$key]=$value
    done
    return 0
}

# Helper to parse the response from the daemon.
# The response is the size in bytes of the bencode-encoded string followed by
# a newline and exactly that many bytes of the string itself.
parse_response() {
    emulate -L zsh
    unsetopt multibyte
    local fd="$1"
    local byte_length
    read -r -u "$fd" byte_length || return 3
    local bytes
    sysread -i "$fd" -c "$byte_length" bytes || return 4
    bencode_decode "$bytes"
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

    local mode="${ZSH_TS_HIGHLIGHTER_MODE:-zsh}"

    if [[ -z "$_ZSH_TS_HIGHLIGHTER_VERSION" ]]; then
        zle -M "zsh-tree-sitter-highlighter: _ZSH_TS_HIGHLIGHTER_VERSION not set, activation may have failed"
        exec {fd}>&-
        return
    fi

    {
        local -A BENCODE_MSG=(
            [version]="$_ZSH_TS_HIGHLIGHTER_VERSION"
            [mode]="$mode"
            [cwd]="$PWD"
            [prebuffer]="$PREBUFFER"
            [buffer]="$BUFFER"
        )
        bencode_encode
    } >&$fd || {
        zle -M "zsh-tree-sitter-highlighter: failed to send request to daemon"
        exec {fd}>&-
        return
    }

    local -A BENCODE_MSG
    parse_response "$fd" || {
        zle -M "zsh-tree-sitter-highlighter: failed to parse response from daemon ($?)"
        exec {fd}>&-
        return
    }

    if [[ "${BENCODE_MSG[status]}" != "ok" ]]; then
        zle -M "zsh-tree-sitter-highlighter: ${BENCODE_MSG[status]}"
        exec {fd}>&-
        return
    fi

    local -a new_regions=()
    for region in "${(@f)BENCODE_MSG[regions]}"; do
        [[ -n "$region" ]] && new_regions+=("$region memo=zsh_ts_highlighter")
    done

    # Dynamic command lookup: check daemon-reported commands against zsh's
    # $functions, $builtins, and $commands.  Unknown commands get an override.
    for cmd in "${(@f)BENCODE_MSG[commands]}"; do
        [[ -z "$cmd" ]] && continue
        local -a cmd_parts=(${=cmd})
        local start="${cmd_parts[1]}"
        local end="${cmd_parts[2]}"
        local name="${cmd_parts[3]}"
        if (( ! $+functions[$name] && ! $+builtins[$name] && ! $+commands[$name] && ! $+aliases[$name] )); then
            new_regions+=("$start $end fg=#f7768e memo=zsh_ts_highlighter")
        fi
    done

    exec {fd}>&-

    region_highlight+=("${new_regions[@]}")
}

if ! zmodload zsh/net/socket 2>/dev/null; then
    print -u2 "zsh-tree-sitter-highlighter: failed to load zsh/net/socket module"
fi

if ! zmodload zsh/system 2>/dev/null; then
    print -u2 "zsh-tree-sitter-highlighter: failed to load zsh/system module"
fi

autoload -U add-zle-hook-widget
add-zle-hook-widget line-pre-redraw _zsh_ts_highlighter

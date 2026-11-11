# Debug widget: show every region_highlight entry with the text it covers.
#
#   source scripts/treeizsh-debug.zsh
#   bindkey '^X^H' treeizsh-debug-regions    # any key; ^Q is often eaten by
#                                             # terminal flow control (stty ixon)

function treeizsh-debug-regions() {
    emulate -L zsh
    local r spec start end text
    local -a lines
    for r in "${region_highlight[@]}"; do
        # Entries look like "[P]start end spec [memo=...]"; offsets are into
        # $BUFFER, or into $PREBUFFER$BUFFER with the P prefix.
        start=${${r%% *}#P}
        end=${${r#* }%% *}
        spec=${${r#* * }% memo=*}
        text=$BUFFER
        [[ $r == P* ]] && text=$PREBUFFER$BUFFER
        lines+=("${(r:5:)start} ${(r:5:)end} ${(r:28:)spec} ${(qq)text[start+1,end]}")
        [[ $r == *memo=* ]] && lines[-1]+="  (${r##* memo=})"
    done
    (( $#lines )) || lines=("(region_highlight is empty)")
    zle -M -- "${(F)lines}"
}
zle -N treeizsh-debug-regions

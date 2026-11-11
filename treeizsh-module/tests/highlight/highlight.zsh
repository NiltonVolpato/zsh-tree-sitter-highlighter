setup() {
  source "$OXIZSH_TEST_SUPPORT/defs.zsh"
  setup_module treeizsh
  zmodload treeizsh || {
      echo "Failed to load treeizsh" >&2
      return 1
  }
  typeset -g TREEIZSH_THEME="${CARGO_MANIFEST_DIR}/tests/highlight/test_theme.toml"
}

test_simple_command() {
  BUFFER='echo hello # comment'
  PREBUFFER=''
  treeizsh_highlight
  print -l "${treeizsh_regions[@]}"
}

test_invalid_command() {
  BUFFER='nonexistent_cmd_12345'
  PREBUFFER=''
  treeizsh_highlight
  print -l "${treeizsh_regions[@]}"
}

test_variable() {
  BUFFER='echo $HOME'
  PREBUFFER=''
  treeizsh_highlight
  print -l "${treeizsh_regions[@]}"
}

test_prebuffer() {
  BUFFER='echo world'
  PREBUFFER='echo hello'
  PREBUFFER=$PREBUFFER$'\n'
  treeizsh_highlight
  print -l "${treeizsh_regions[@]}"
}

test_complex_nesting() {
  BUFFER='echo "Hello $(function name() { if [ -z "$1" ]; then echo "World"; else echo "${1}"; fi }; name "$USER") on $(uname)\!"'
  PREBUFFER=''
  treeizsh_highlight
  print -l "${treeizsh_regions[@]}"
}

test_markdown_heading() {
  BUFFER='# Hello World'
  PREBUFFER=''
  typeset -g TREEIZSH_MODE='markdown'
  treeizsh_highlight
  print -l "${treeizsh_regions[@]}"
  typeset -g TREEIZSH_MODE='zsh'
}

test_markdown_bold_italic() {
  BUFFER='**bold** and *italic*'
  PREBUFFER=''
  typeset -g TREEIZSH_MODE='markdown'
  treeizsh_highlight
  print -l "${treeizsh_regions[@]}"
  typeset -g TREEIZSH_MODE='zsh'
}

test_markdown_code_block() {
  BUFFER=$'```zsh\necho hello\n```'
  PREBUFFER=''
  typeset -g TREEIZSH_MODE='markdown'
  treeizsh_highlight
  print -l "${treeizsh_regions[@]}"
  typeset -g TREEIZSH_MODE='zsh'
}

test_markdown_multiline() {
  BUFFER="\
# Heading

- **bold** and *italic*
- inline \`code\`
- block:

\`\`\`zsh
echo hello
\`\`\`
"
  PREBUFFER=''
  typeset -g TREEIZSH_MODE='markdown'
  treeizsh_highlight
  print -l "${treeizsh_regions[@]}"
  typeset -g TREEIZSH_MODE='zsh'
}

test_unchanged_input_reuses_regions() {
  treeizsh_highlight --reset
  BUFFER='echo hi'
  PREBUFFER=''
  treeizsh_highlight
  print "changed=$treeizsh_changed"; print -l "${treeizsh_regions[@]}"
  treeizsh_highlight
  print "changed=$treeizsh_changed"; print -l "${treeizsh_regions[@]}"
  BUFFER='echo hi!'
  treeizsh_highlight
  print "changed=$treeizsh_changed"
}

test_prebuffer_split_is_part_of_input() {
  # Same full source, different split: the regions must be recomputed.
  treeizsh_highlight --reset
  PREBUFFER=$'echo a\n'
  BUFFER='ls'
  treeizsh_highlight
  print "changed=$treeizsh_changed"; print -l "${treeizsh_regions[@]}"
  PREBUFFER=''
  BUFFER=$'echo a\nls'
  treeizsh_highlight
  print "changed=$treeizsh_changed"; print -l "${treeizsh_regions[@]}"
}

test_reset_rechecks_commands() {
  treeizsh_highlight --reset
  PREBUFFER=''
  BUFFER='defined_later_xyz'
  treeizsh_highlight
  print -l "${treeizsh_regions[@]}"
  defined_later_xyz() { true; }
  # Cached within the same command line...
  BUFFER='defined_later_xyz '
  treeizsh_highlight
  print -l "${treeizsh_regions[@]}"
  # ...and rechecked on the next one.
  treeizsh_highlight --reset
  treeizsh_highlight
  print -l "${treeizsh_regions[@]}"
  unfunction defined_later_xyz
}

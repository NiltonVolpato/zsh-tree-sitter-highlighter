setup() {
  source "$ZSH_MODULE_TEST_SUPPORT/defs.zsh"
  setup_module treeish
  zmodload treeish || {
      echo "Failed to load treeish" >&2
      return 1
  }
  typeset -g TREEISH_THEME="tests/highlight/test_theme.toml"
  rehash
  # FIXME: This hash call shouldn't be needed, but it works around a bug in
  # zsh-module's CommandsTable::contains_key implementation (which returns
  # false for commands that exist in cmdnamtab but haven't been resolved yet).
  hash uname
}

test_simple_command() {
  BUFFER='echo hello # comment'
  PREBUFFER=''
  treeish_highlight
  print -l "${treeish_regions[@]}"
}

test_invalid_command() {
  BUFFER='nonexistent_cmd_12345'
  PREBUFFER=''
  treeish_highlight
  print -l "${treeish_regions[@]}"
}

test_variable() {
  BUFFER='echo $HOME'
  PREBUFFER=''
  treeish_highlight
  print -l "${treeish_regions[@]}"
}

test_prebuffer() {
  BUFFER='echo world'
  PREBUFFER='echo hello'
  PREBUFFER=$PREBUFFER$'\n'
  treeish_highlight
  print -l "${treeish_regions[@]}"
}

test_complex_nesting() {
  BUFFER='echo "Hello $(function name() { if [ -z "$1" ]; then echo "World"; else echo "${1}"; fi }; name "$USER") on $(uname)\!"'
  PREBUFFER=''
  treeish_highlight
  print -l "${treeish_regions[@]}"
}

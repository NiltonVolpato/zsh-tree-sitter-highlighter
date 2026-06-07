setup() {
  source "$ZSH_MODULE_TEST_SUPPORT/defs.zsh"
  setup_module zsh-ts-module
  zmodload zsh_ts_module || {
      echo "Failed to load zsh_ts_module" >&2
      return 1
  }
  typeset -g _ZSH_TS_HIGHLIGHTER_THEME="tests/highlight/test_theme.toml"
  rehash
  # FIXME: This hash call shouldn't be needed, but it works around a bug in
  # zsh-module's CommandsTable::contains_key implementation (which returns
  # false for commands that exist in cmdnamtab but haven't been resolved yet).
  hash uname
}

test_simple_command() {
  BUFFER='echo hello # comment'
  PREBUFFER=''
  zsh_ts_highlight
  print -l "${_zsh_ts_regions[@]}"
}

test_invalid_command() {
  BUFFER='nonexistent_cmd_12345'
  PREBUFFER=''
  zsh_ts_highlight
  print -l "${_zsh_ts_regions[@]}"
}

test_variable() {
  BUFFER='echo $HOME'
  PREBUFFER=''
  zsh_ts_highlight
  print -l "${_zsh_ts_regions[@]}"
}

test_prebuffer() {
  BUFFER='echo world'
  PREBUFFER='echo hello'
  PREBUFFER=$PREBUFFER$'\n'
  zsh_ts_highlight
  print -l "${_zsh_ts_regions[@]}"
}

test_complex_nesting() {
  BUFFER='echo "Hello $(function name() { if [ -z "$1" ]; then echo "World"; else echo "${1}"; fi }; name "$USER") on $(uname)\!"'
  PREBUFFER=''
  zsh_ts_highlight
  print -l "${_zsh_ts_regions[@]}"
}

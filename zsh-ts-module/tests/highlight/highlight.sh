setup() {
  source "$ZSH_MODULE_TEST_SUPPORT/defs.zsh"
  setup_module zsh-ts-module
  zmodload zsh_ts_module || {
      echo "Failed to load zsh_ts_module" >&2
      return 1
  }
  typeset -gA _ZSH_TS_HIGHLIGHTER_THEME=(
      [comment]=fg=#565f89
      [string]=fg=#e0af68
      [function]=fg=#7aa2f7
      [command.invalid]=fg=#f7768e,bold
      [variable]=fg=#e0af68
      [operator]=fg=#89ddff
  )
}

test_simple_command() {
  BUFFER='echo hello # comment'
  PREBUFFER=''
  zsh_ts_highlight
  for r in "${_zsh_ts_regions[@]}"; do
    echo "region: $r"
  done
}

test_invalid_command() {
  BUFFER='nonexistent_cmd_12345'
  PREBUFFER=''
  zsh_ts_highlight
  for r in "${_zsh_ts_regions[@]}"; do
    echo "region: $r"
  done
}

test_variable() {
  BUFFER='echo $HOME'
  PREBUFFER=''
  zsh_ts_highlight
  for r in "${_zsh_ts_regions[@]}"; do
    echo "region: $r"
  done
}

test_prebuffer() {
  BUFFER='echo world'
  PREBUFFER='echo hello'
  PREBUFFER=$PREBUFFER$'\n'
  zsh_ts_highlight
  for r in "${_zsh_ts_regions[@]}"; do
    echo "region: $r"
  done
}

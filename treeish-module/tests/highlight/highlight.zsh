setup() {
  source "$ZSH_MODULE_TEST_SUPPORT/defs.zsh"
  setup_module treeish
  zmodload treeish || {
      echo "Failed to load treeish" >&2
      return 1
  }
  typeset -g TREEISH_THEME="${CARGO_MANIFEST_DIR}/tests/highlight/test_theme.toml"
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

test_markdown_heading() {
  BUFFER='# Hello World'
  PREBUFFER=''
  typeset -g TREEISH_MODE='markdown'
  treeish_highlight
  print -l "${treeish_regions[@]}"
  typeset -g TREEISH_MODE='zsh'
}

test_markdown_bold_italic() {
  BUFFER='**bold** and *italic*'
  PREBUFFER=''
  typeset -g TREEISH_MODE='markdown'
  treeish_highlight
  print -l "${treeish_regions[@]}"
  typeset -g TREEISH_MODE='zsh'
}

test_markdown_code_block() {
  BUFFER=$'```zsh\necho hello\n```'
  PREBUFFER=''
  typeset -g TREEISH_MODE='markdown'
  treeish_highlight
  print -l "${treeish_regions[@]}"
  typeset -g TREEISH_MODE='zsh'
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
  typeset -g TREEISH_MODE='markdown'
  treeish_highlight
  print -l "${treeish_regions[@]}"
  typeset -g TREEISH_MODE='zsh'
}

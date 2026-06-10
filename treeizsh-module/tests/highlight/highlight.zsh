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

use tests::{highlight_markup, highlight_markup_in_mode};

#[test]
fn test_markdown_heading() {
    assert_eq!(
        highlight_markup_in_mode("# Hello World", "markdown"),
        "<punctuation.special>#</punctuation.special> <text.title>Hello World</text.title>"
    );
}

#[test]
fn test_markdown_bold_italic() {
    assert_eq!(
        highlight_markup_in_mode("**bold** and *italic*", "markdown"),
        "<punctuation.delimiter>**</punctuation.delimiter><text.strong>bold</text.strong><punctuation.delimiter>**</punctuation.delimiter> and <punctuation.delimiter>*</punctuation.delimiter><text.emphasis>italic</text.emphasis><punctuation.delimiter>*</punctuation.delimiter>"
    );
}

#[test]
fn test_markdown_multiline() {
    let multiline_buffer = "\
# Heading

- **bold** and *italic*
- inline `code`
- block:

```zsh
echo hello
```
";
    assert_eq!(
        highlight_markup_in_mode(multiline_buffer, "markdown"),
        "\
<punctuation.special>#</punctuation.special> <text.title>Heading</text.title>

<punctuation.special>- </punctuation.special><punctuation.delimiter>**</punctuation.delimiter><text.strong>bold</text.strong><punctuation.delimiter>**</punctuation.delimiter> and <punctuation.delimiter>*</punctuation.delimiter><text.emphasis>italic</text.emphasis><punctuation.delimiter>*</punctuation.delimiter>
<punctuation.special>- </punctuation.special>inline <punctuation.delimiter>`</punctuation.delimiter><text.literal>code</text.literal><punctuation.delimiter>`</punctuation.delimiter>
<punctuation.special>- </punctuation.special>block:

<punctuation.delimiter>```</punctuation.delimiter><text.literal>zsh</text.literal>
<function>echo</function><text.literal> hello</text.literal>
<punctuation.delimiter>```</punctuation.delimiter>
"
    );
}

#[test]
fn test_simple_command_with_comment() {
    assert_eq!(
        highlight_markup("echo hello # comment"),
        "<function>echo</function> hello <comment># comment</comment>"
    );
}

#[test]
fn test_invalid_command() {
    assert_eq!(
        highlight_markup("nonexistent_cmd_12345"),
        "<command.invalid>nonexistent_cmd_12345</command.invalid>"
    );
}

#[test]
fn test_variable() {
    assert_eq!(
        highlight_markup("echo $HOME"),
        "<function>echo</function> <variable>$HOME</variable>"
    );
}

#[test]
fn test_double_quoted_string() {
    assert_eq!(
        highlight_markup("echo \"hello\""),
        "<function>echo</function> <string>\"hello\"</string>"
    );
}

#[test]
fn test_single_quoted_string() {
    assert_eq!(
        highlight_markup("echo 'world'"),
        "<function>echo</function> <string>'world'</string>"
    );
}

#[test]
fn test_keywords_control_structures() {
    assert_eq!(
        highlight_markup("if true; then echo yes; fi"),
        "<keyword>if</keyword> <function>true</function>; <keyword>then</keyword> <function>echo</function> yes; <keyword>fi</keyword>"
    );
}


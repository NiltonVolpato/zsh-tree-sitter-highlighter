use tests::highlight_markup;

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

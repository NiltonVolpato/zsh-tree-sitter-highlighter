use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    ExpectedEquals,
    ExpectedColon,
    InvalidLength,
    UnexpectedEof,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::ExpectedEquals => write!(f, "expected '=' in request"),
            ParseError::ExpectedColon => write!(f, "expected ':' in request"),
            ParseError::InvalidLength => write!(f, "invalid length in request"),
            ParseError::UnexpectedEof => write!(f, "unexpected end of input"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Read `key=length:value` records from a stream until the `EOM` key is seen.
///
/// This is designed for the daemon: it reads records incrementally without
/// waiting for EOF, so the client can keep the connection open for the response.
/// The client sends `EOM=0:` as an explicit end-of-message sentinel.
pub fn read_request<R: Read>(reader: &mut BufReader<R>) -> Result<HashMap<String, String>, ReadError> {
    let mut map = HashMap::new();
    let mut buf = String::new();

    loop {
        buf.clear();
        // Read one line (the key=length: prefix and possibly part of the value)
        let n = reader.read_line(&mut buf).map_err(ReadError::Io)?;
        if n == 0 {
            break; // EOF
        }

        // Trim trailing newline (Unix domain socket, always \n)
        if buf.ends_with('\n') {
            buf.pop();
        }

        // Parse key=length: from the buffer, producing owned values
        let (key, rest) = buf
            .split_once('=')
            .ok_or(ReadError::Parse(ParseError::ExpectedEquals))?;
        let key = key.to_string();
        let (len_str, value_start) = rest
            .split_once(':')
            .ok_or(ReadError::Parse(ParseError::ExpectedColon))?;
        let length: usize = len_str
            .parse()
            .map_err(|_| ReadError::Parse(ParseError::InvalidLength))?;
        let value_start = value_start.to_string();

        // The value starts after ':' on the current line.
        // We need exactly `length` chars total for the value.
        let value_start_chars = value_start.chars().count();

        // Stop on explicit end-of-message sentinel (check before inserting into map)
        if key == "EOM" {
            break;
        }

        if value_start_chars >= length {
            // Entire value is on this line
            let value: String = value_start.chars().take(length).collect();
            map.insert(key, value);
        } else {
            // Value spans multiple lines.
            // The newline stripped by read_line is part of the value.
            let mut value = value_start;
            value.push('\n');
            // We now have value_start_chars + 1 chars. Keep reading until we have `length`.
            while value.chars().count() < length {
                buf.clear();
                let n = reader.read_line(&mut buf).map_err(ReadError::Io)?;
                if n == 0 {
                    return Err(ReadError::Parse(ParseError::UnexpectedEof));
                }
                // Trim trailing newline (Unix domain socket, always \n)
                if buf.ends_with('\n') {
                    buf.pop();
                }
                let line_chars = buf.chars().count();
                let have = value.chars().count();
                let need = length - have;
                if line_chars <= need {
                    // Take the whole line content
                    value.push_str(&buf);
                    // If we still need more, the newline we stripped is part of the value
                    if value.chars().count() < length {
                        value.push('\n');
                    }
                } else {
                    // Take only what we need from this line
                    let taken: String = buf.chars().take(need).collect();
                    value.push_str(&taken);
                }
            }
            // Trim to exact length (in case we overshot with a newline)
            let v: String = value.chars().take(length).collect();
            map.insert(key, v);
        }
    }

    Ok(map)
}

#[derive(Debug)]
pub enum ReadError {
    Io(std::io::Error),
    Parse(ParseError),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Io(e) => write!(f, "I/O error: {e}"),
            ReadError::Parse(e) => write!(f, "parse error: {e}"),
        }
    }
}

impl std::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ReadError::Io(e) => Some(e),
            ReadError::Parse(e) => Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(input: &str) -> HashMap<String, String> {
        let mut reader = BufReader::new(input.as_bytes());
        read_request(&mut reader).unwrap()
    }

    #[test]
    fn basic_request() {
        let input = "ver=1:2\nlang=3:zsh\npwd=5:/home\nprebuffer=0:\nbuffer=10:echo hello\nEOM=0:\n";
        let map = read(input);
        assert_eq!(map["ver"], "2");
        assert_eq!(map["lang"], "zsh");
        assert_eq!(map["pwd"], "/home");
        assert_eq!(map["prebuffer"], "");
        assert_eq!(map["buffer"], "echo hello");
    }

    #[test]
    fn empty_value() {
        let input = "prebuffer=0:\nEOM=0:\n";
        let map = read(input);
        assert_eq!(map["prebuffer"], "");
    }

    #[test]
    fn spaces_in_value() {
        let input = "buffer=10:echo hello\nEOM=0:\n";
        let map = read(input);
        assert_eq!(map["buffer"], "echo hello");
    }

    #[test]
    fn newline_in_value() {
        // "foo\nbar" = 7 chars
        let input = "buffer=7:foo\nbar\nEOM=0:\n";
        let map = read(input);
        assert_eq!(map["buffer"], "foo\nbar");
    }

    #[test]
    fn embedded_quotes_and_special_chars() {
        // "it's here" = 9 chars
        let input = "buffer=9:it's here\nEOM=0:\n";
        let map = read(input);
        assert_eq!(map["buffer"], "it's here");
    }

    #[test]
    fn path_value() {
        let input = "pwd=29:/Users/nilton/tmp/shell-words\nEOM=0:\n";
        let map = read(input);
        assert_eq!(map["pwd"], "/Users/nilton/tmp/shell-words");
    }

    #[test]
    fn key_order_does_not_matter() {
        let input = "buffer=4:echo\nver=1:2\nlang=3:zsh\nEOM=0:\n";
        let map = read(input);
        assert_eq!(map["buffer"], "echo");
        assert_eq!(map["ver"], "2");
        assert_eq!(map["lang"], "zsh");
    }

    #[test]
    fn value_with_newlines_and_quotes() {
        // "foo\nbar'baz z\"zzz" = 17 chars
        let input = "buffer=17:foo\nbar'baz z\"zzz\nEOM=0:\n";
        let map = read(input);
        assert_eq!(map["buffer"], "foo\nbar'baz z\"zzz");
    }

    #[test]
    fn missing_equals() {
        let input = "ver1:2\nEOM=0:\n";
        assert!(read_request(&mut BufReader::new(input.as_bytes())).is_err());
    }

    #[test]
    fn missing_colon() {
        let input = "ver=12\nEOM=0:\n";
        assert!(read_request(&mut BufReader::new(input.as_bytes())).is_err());
    }

    #[test]
    fn invalid_length() {
        let input = "ver=abc:2\nEOM=0:\n";
        assert!(read_request(&mut BufReader::new(input.as_bytes())).is_err());
    }

    #[test]
    fn truncated_value_consumes_next_record() {
        // A truncated value (length > actual chars) will consume subsequent
        // records as value content. This is inherent to the length-prefixed
        // format — the parser can't distinguish value content from records.
        // The client is responsible for sending correct lengths.
        let input = "buffer=10:hi\nEOM=0:\n";
        let map = read(input);
        // "hi" (2) + "\n" (1) + "EOM=0:" (6) + "\n" (1) = 10 chars
        assert_eq!(map["buffer"], "hi\nEOM=0:\n");
        // EOM was consumed as value content, so the reader hits EOF and returns Ok
    }

    #[test]
    fn empty_input() {
        let map = read("");
        assert!(map.is_empty());
    }

    #[test]
    fn eom_is_terminator() {
        // After seeing EOM, the reader stops even if there's more data
        let input = "ver=1:2\nbuffer=4:echo\nEOM=0:\nextra=4:data\n";
        let map = read(input);
        assert_eq!(map.len(), 2);
        assert_eq!(map["ver"], "2");
        assert_eq!(map["buffer"], "echo");
        assert!(!map.contains_key("extra"));
    }

    #[test]
    fn blank_line_between_records_is_rejected() {
        // A blank line between records is malformed, not silently skipped.
        // Before the fix, the blank line was swallowed and the parser would
        // return Ok with whatever it had collected so far.
        let input = "ver=1:2\n\nbuffer=4:echo\nEOM=0:\n";
        assert!(read_request(&mut BufReader::new(input.as_bytes())).is_err());
    }

    #[test]
    fn blank_line_within_value_is_preserved() {
        // "foo\n\nbar\n" = 9 chars — two consecutive newlines inside the value,
        // plus a trailing newline. The inner collector must preserve blank lines
        // as actual value content, not skip them.
        let input = "buffer=9:foo\n\nbar\nEOM=0:\n";
        let map = read(input);
        assert_eq!(map["buffer"], "foo\n\nbar\n");
    }

    /// Regression test: multi-byte (UTF-8) characters must be counted by
    /// char, not by byte. If `.chars().count()` regressed to `.len()`, the
    /// parser would read too few bytes and fail to reconstruct the value.
    #[test]
    fn multibyte_value_char_counting() {
        // "café" = 4 chars, 5 bytes (é = 0xC3 0xA9)
        let input = "buffer=4:café\nEOM=0:\n";
        let map = read(input);
        assert_eq!(map["buffer"], "café");
    }

    #[test]
    fn multibyte_value_across_lines() {
        // "αβ\nγδ" = 5 chars (α=2 bytes, β=2 bytes, \n=1 byte, γ=2 bytes, δ=2 bytes)
        let input = "buffer=5:αβ\nγδ\nEOM=0:\n";
        let map = read(input);
        assert_eq!(map["buffer"], "αβ\nγδ");
    }

    #[test]
    fn multibyte_emoji_value() {
        // "🎉🚀" = 2 chars, 8 bytes
        let input = "buffer=2:🎉🚀\nEOM=0:\n";
        let map = read(input);
        assert_eq!(map["buffer"], "🎉🚀");
    }
}

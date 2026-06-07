use expectrl::process::Termios;
use expectrl::session::OsSession;
use expectrl::{Expect, Regex, Session};
use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn color_to_tag(color: u8) -> &'static str {
    match color {
        17 => "comment",
        18 => "constant",
        19 => "embedded",
        20 => "function",
        21 => "keyword",
        22 => "number",
        23 => "operator",
        24 => "property",
        25 => "punctuation.delimiter",
        26 => "punctuation.special",
        27 => "string",
        28 => "string.escape",
        29 => "text.emphasis",
        30 => "text.literal",
        31 => "text.reference",
        32 => "text.strong",
        33 => "text.title",
        34 => "text.uri",
        35 => "variable",
        36 => "command.invalid",
        37 => "path",
        38 => "path.directory",
        _ => "unknown",
    }
}

fn parse_zle_highlight(stream: &str) -> String {
    let prompt = "READY_PROMPT ";
    let idx = match stream.rfind(prompt) {
        Some(i) => i + prompt.len(),
        None => 0,
    };
    let content = &stream[idx..];

    let mut chars = content.chars().peekable();
    let mut current_color: Option<u8> = None;
    let mut result = String::new();

    while let Some(&c) = chars.peek() {
        if c == '\x1b' {
            chars.next();
            if chars.peek() == Some(&'[') {
                chars.next();
                let mut seq = String::new();
                while let Some(&nc) = chars.peek() {
                    if nc.is_ascii_alphabetic() {
                        let term = nc;
                        chars.next();

                        if term == 'm' {
                            let parts: Vec<&str> = if seq.contains(':') {
                                seq.split(':').collect()
                            } else {
                                seq.split(';').collect()
                            };

                            if parts.len() >= 3 && parts[0] == "38" && parts[1] == "5" {
                                if let Ok(val) = parts[2].parse::<u8>() {
                                    if current_color != Some(val) {
                                        if let Some(prev) = current_color {
                                            result.push_str(&format!("</{}>", color_to_tag(prev)));
                                        }
                                        current_color = Some(val);
                                        result.push_str(&format!("<{}>", color_to_tag(val)));
                                    }
                                }
                            } else if seq == "39" || seq == "0" || seq.is_empty() {
                                if let Some(prev) = current_color {
                                    result.push_str(&format!("</{}>", color_to_tag(prev)));
                                    current_color = None;
                                }
                            }
                        }
                        break;
                    } else {
                        seq.push(nc);
                        chars.next();
                    }
                }
            }
        } else {
            let nc = chars.next().unwrap();
            if nc != '\r' && nc != '\n' {
                result.push(nc);
            }
        }
    }

    if let Some(prev) = current_color {
        result.push_str(&format!("</{}>", color_to_tag(prev)));
    }

    result.trim().to_string()
}

fn expect_pattern(session: &mut OsSession, pattern: &str, timeout: Duration) -> Vec<u8> {
    let deadline = std::time::Instant::now() + timeout;
    let needle = Regex(pattern);
    loop {
        match session.check(&needle) {
            Ok(captures) if !captures.is_empty() => {
                let mut data = captures.before().to_vec();
                if let Some(mat) = captures.get(0) {
                    data.extend_from_slice(mat);
                }
                return data;
            }
            Ok(_) => {
                if std::time::Instant::now() >= deadline {
                    panic!("Timeout waiting for pattern: {:?}", pattern);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("Error checking pattern: {:?}", e),
        }
    }
}

fn read_available(session: &mut OsSession) -> String {
    let mut result = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(20));
        match session.check(Regex(r"(?s).+")) {
            Ok(captures) if !captures.is_empty() => {
                let mut data = captures.before().to_vec();
                if let Some(matched) = captures.get(0) {
                    data.extend_from_slice(matched);
                }
                result.push_str(&String::from_utf8_lossy(&data));
            }
            _ => break,
        }
    }
    result
}

#[test]
fn test_zle_end_to_end_highlighting() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.parent().unwrap();
    let target_debug = workspace_root.join("target/debug");
    let theme_path = manifest_dir.join("highlight/test_ansidecode_theme.toml");
    let integration_script = workspace_root.join("zsh-ts-module/zsh-integration.zsh");

    // Spawn Zsh with TERM=xterm-256color set in environment at startup
    let mut cmd = Command::new("zsh");
    cmd.arg("-df");
    cmd.env("TERM", "xterm-256color");
    cmd.env("PROMPT", "READY_PROMPT ");

    let mut session = Session::spawn(cmd).expect("Failed to spawn zsh");
    session.set_echo(false).expect("Failed to set echo");

    // Set terminal window size to 80x24 so ZLE is active and works properly
    session
        .get_process_mut()
        .set_window_size(80, 24)
        .expect("Failed to set window size");

    session.expect(Regex("READY_PROMPT ")).unwrap();

    session
        .send_line(format!("module_path+=({:?})", target_debug))
        .unwrap();
    session.expect(Regex("READY_PROMPT ")).unwrap();

    session.write_all(b"zmodload zsh_ts_module\r").unwrap();
    session.flush().unwrap();
    expect_pattern(&mut session, "READY_PROMPT ", Duration::from_secs(2));

    // Source integration script
    let cmd_src = format!("source {:?}\r", integration_script.to_str().unwrap());
    session.write_all(cmd_src.as_bytes()).unwrap();
    session.flush().unwrap();
    expect_pattern(&mut session, "READY_PROMPT ", Duration::from_secs(2));

    // Set theme path
    let cmd_theme = format!(
        "typeset -g _ZSH_TS_HIGHLIGHTER_THEME={:?}\r",
        theme_path.to_str().unwrap()
    );
    session.write_all(cmd_theme.as_bytes()).unwrap();
    session.flush().unwrap();
    expect_pattern(&mut session, "READY_PROMPT ", Duration::from_secs(2));

    // Rehash & Workaround
    session.write_all(b"rehash\r").unwrap();
    session.flush().unwrap();
    expect_pattern(&mut session, "READY_PROMPT ", Duration::from_secs(2));
    session.write_all(b"hash uname\r").unwrap();
    session.flush().unwrap();
    expect_pattern(&mut session, "READY_PROMPT ", Duration::from_secs(2));

    // Helper closure to assert ZLE highlighting markup
    let assert_highlight = |session: &mut OsSession, buffer: &str, expected_markup: &str| {
        // Sleep and purge
        std::thread::sleep(Duration::from_millis(50));
        let _ = read_available(session);

        // Send input buffer and Ctrl-L (redraw)
        session.write_all(buffer.as_bytes()).unwrap();
        session.write_all(b"\x0c").unwrap();
        session.flush().unwrap();

        // Wait for ZLE redraw to finish rendering
        std::thread::sleep(Duration::from_millis(200));

        // Read all available bytes
        let captured = read_available(session);
        println!("RAW CAPTURED FOR {:?}:\n{:?}", buffer, captured);

        // Cancel line with Ctrl-C to clear buffer and sync back to prompt
        session.write_all(b"\x03").unwrap();
        session.flush().unwrap();
        expect_pattern(session, "READY_PROMPT ", Duration::from_secs(2));

        let parsed = parse_zle_highlight(&captured);
        assert_eq!(
            parsed, expected_markup,
            "Failed highlighting for buffer: {:?}",
            buffer
        );
    };

    // 1. Simple command with comment
    assert_highlight(
        &mut session,
        "echo hello # comment",
        "<function>echo</function> hello <comment># comment</comment>",
    );
}

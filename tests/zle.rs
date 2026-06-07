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
    let prompt = PROMPT;
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

const PROMPT: &str = "READY_PROMPT ";

#[test]
fn test_zle_end_to_end_highlighting() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.parent().unwrap();
    let target_debug = workspace_root.join("target/debug");
    let theme_path = manifest_dir.join("highlight/test_ansidecode_theme.toml");
    let integration_script = workspace_root.join("zsh-ts-module/zsh-integration.zsh");

    // Use ZDOTDIR trick to load prompt and module automatically
    let temp_dir = tempfile::tempdir().unwrap();
    let zshrc_content = format!(
        r#"
PROMPT='{}'
module_path+=({:?})
zmodload zsh_ts_module
zmodload zsh/zle
source {:?}
typeset -g _ZSH_TS_HIGHLIGHTER_THEME={:?}
abort_command() {{ zle -I; BUFFER="" }}
zle -N abort_command; bindkey "^G" abort_command
rehash
hash uname
    "#,
        PROMPT, target_debug, integration_script, theme_path
    );
    let zshrc_path = temp_dir.path().join(".zshrc");
    std::fs::write(&zshrc_path, zshrc_content).unwrap();

    let mut cmd = Command::new("zsh");
    cmd.arg("-d"); // user rc files in ZDOTDIR still run
    cmd.env("TERM", "xterm-256color");
    cmd.env("ZDOTDIR", temp_dir.path());

    let mut session = Session::spawn(cmd).expect("Failed to spawn zsh");
    session.set_echo(false).expect("Failed to set echo");

    // Set terminal window size to 80x24 so ZLE is active and works properly
    session
        .get_process_mut()
        .set_window_size(80, 24)
        .expect("Failed to set window size");

    // Wait for the prompt which confirms Zsh initialized and loaded everything
    session.expect(Regex(PROMPT)).unwrap();

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
        session.expect(Regex(PROMPT)).unwrap();

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

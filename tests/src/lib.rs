use expectrl::process::Termios;
use expectrl::session::OsProcess;
use expectrl::session::OsSession;
use expectrl::{Expect, Regex, Session};
use std::env;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

const PROMPT_RE: Regex<&str> = Regex(r"READY \[([0-9]+)\] % ");

fn check_result<S: Read>(
    session: &mut Session<OsProcess, S>,
    result: Result<expectrl::Captures, expectrl::Error>,
) -> expectrl::Captures {
    let captures = match result {
        Ok(captures) => captures,
        Err(e) => {
            let mut output = String::new();
            let _ = session.read_to_string(&mut output);
            panic!(
                "Zsh session failed (expect prompt): {:?}. Session output: {:?}",
                e, output
            );
        }
    };

    if let Some(code_bytes) = captures.get(1) {
        let code_str = String::from_utf8_lossy(code_bytes);
        if let Ok(code) = code_str.parse::<i32>() {
            if code != 0 {
                let mut output = String::new();
                let _ = session.read_to_string(&mut output);
                panic!(
                    "Zsh command failed with exit code {}. Session output: {:?}",
                    code, output
                );
            }
        }
    }

    captures
}

pub fn color_to_tag(color: u8) -> &'static str {
    match color {
        17 => "comment",
        46 => "constant",
        196 => "embedded",
        45 => "function",
        201 => "keyword",
        94 => "number",
        226 => "operator",
        16 => "property",
        198 => "punctuation.delimiter",
        27 => "punctuation.special",
        49 => "string",
        229 => "string.escape",
        208 => "text.emphasis",
        29 => "text.literal",
        32 => "text.reference",
        213 => "text.strong",
        70 => "text.title",
        21 => "text.uri",
        225 => "variable",
        95 => "command.invalid",
        160 => "error",
        130 => "path",
        51 => "path.directory",
        _ => "unknown",
    }
}

pub fn tag_to_color(tag: &str) -> u8 {
    match tag {
        "comment" => 17,
        "constant" => 46,
        "embedded" => 196,
        "function" => 45,
        "keyword" => 201,
        "number" => 94,
        "operator" => 226,
        "property" => 16,
        "punctuation.delimiter" => 198,
        "punctuation.special" => 27,
        "string" => 49,
        "string.escape" => 229,
        "text.emphasis" => 208,
        "text.literal" => 29,
        "text.reference" => 32,
        "text.strong" => 213,
        "text.title" => 70,
        "text.uri" => 21,
        "variable" => 225,
        "command.invalid" => 95,
        "error" => 160,
        "path" => 130,
        "path.directory" => 51,
        _ => 15,
    }
}

pub fn parse_spans(stream: &str) -> (String, Vec<(usize, usize, &'static str)>) {
    let content = stream;

    // Filter out carriage returns
    let filtered: String = content.chars().filter(|&c| c != '\r').collect();

    // Regex matches any CSI escape sequence: \x1b[ <parameters> <letter>
    let ansi_re = regex::Regex::new(r"\x1b\[([0-9;:]*)([a-zA-Z])").unwrap();

    let mut clean_text = String::new();
    let mut spans = Vec::new();
    let mut last_idx = 0;
    let mut current_color: Option<u8> = None;
    let mut current_span_start = 0;

    // CSI (Control Sequence Introducer) Graphics / Color setting command
    const SGR_COMMAND: &str = "m";
    // SGR parameter prefix for setting 256-color foreground
    const FG_256_COLOR_PREFIX: &str = "38";
    const COLOR_TYPE_256: &str = "5";
    // SGR parameter for resetting default foreground color
    const FG_RESET: &str = "39";
    // SGR parameter for resetting all attributes
    const RESET_ALL: &str = "0";

    for mat in ansi_re.find_iter(&filtered) {
        let text_chunk = &filtered[last_idx..mat.start()];
        clean_text.push_str(text_chunk);

        let caps = ansi_re.captures(mat.as_str()).unwrap();
        let params = caps.get(1).unwrap().as_str();
        let term = caps.get(2).unwrap().as_str();

        last_idx = mat.end();

        if term != SGR_COMMAND {
            continue;
        }
        let parts: Vec<&str> = if params.contains(':') {
            params.split(':').collect()
        } else {
            params.split(';').collect()
        };

        let next_color =
            if parts.len() >= 3 && parts[0] == FG_256_COLOR_PREFIX && parts[1] == COLOR_TYPE_256 {
                parts[2].parse::<u8>().ok()
            } else if params == FG_RESET || params == RESET_ALL || params.is_empty() {
                None
            } else {
                current_color
            };

        if next_color != current_color {
            let char_count = clean_text.chars().count();
            if let Some(prev) = current_color {
                spans.push((current_span_start, char_count, color_to_tag(prev)));
            }
            current_color = next_color;
            current_span_start = char_count;
        }
    }

    clean_text.push_str(&filtered[last_idx..]);
    if let Some(prev) = current_color {
        let char_count = clean_text.chars().count();
        spans.push((current_span_start, char_count, color_to_tag(prev)));
    }

    let mut clean_text = clean_text;
    assert!(
        clean_text.ends_with('\n'),
        "ZLE captured buffer must end with a newline character introduced by the terminal redraw"
    );
    clean_text.pop();

    let clean_text_len = clean_text.chars().count();
    let mut final_spans = Vec::new();
    for (start, end, tag) in spans {
        if start < clean_text_len {
            final_spans.push((start, end.min(clean_text_len), tag));
        }
    }

    (clean_text, final_spans)
}

pub fn spans_to_markup(clean_text: &str, spans: &[(usize, usize, &'static str)]) -> String {
    let chars: Vec<char> = clean_text.chars().collect();
    let mut result = String::new();
    let mut curr = 0;
    for &(start, end, tag) in spans {
        while curr < start && curr < chars.len() {
            result.push(chars[curr]);
            curr += 1;
        }
        result.push_str(&format!("<{}>", tag));
        while curr < end && curr < chars.len() {
            result.push(chars[curr]);
            curr += 1;
        }
        result.push_str(&format!("</{}>", tag));
    }
    while curr < chars.len() {
        result.push(chars[curr]);
        curr += 1;
    }
    result
}

pub fn parse_zle_highlight(stream: &str) -> String {
    let (clean_text, spans) = parse_spans(stream);
    spans_to_markup(&clean_text, &spans)
}

pub fn render_diagram(buffer: &str, spans: &[(usize, usize, &'static str)]) -> String {
    let chars: Vec<char> = buffer.chars().collect();

    struct Interval {
        start: usize,
        end: usize,
        col: usize,
        label: String,
    }

    let mut intervals = Vec::new();
    for &(start, end, tag) in spans {
        if start >= end || start >= chars.len() {
            continue;
        }
        let text: String = chars[start..end.min(chars.len())].iter().collect();
        let col = start + (end - start) / 2;
        let label = format!("{}: {:?}", tag, text);
        intervals.push(Interval {
            start,
            end,
            col,
            label,
        });
    }

    intervals.sort_by_key(|inv| inv.start);

    let n = intervals.len();
    if n == 0 {
        return buffer.to_string();
    }

    let mut lines = Vec::new();

    for k in 0..n {
        let mut line = String::new();
        let target_idx = k;

        let mut curr_col = 0;
        for j in 0..=target_idx {
            let inv_col = intervals[j].col;
            while curr_col < inv_col {
                line.push(' ');
                curr_col += 1;
            }

            if j == target_idx {
                line.push_str("+- ");
                line.push_str(&intervals[j].label);
                break;
            } else {
                line.push('|');
                curr_col += 1;
            }
        }
        lines.push(line);
    }

    let mut conn_line = String::new();
    let mut curr_col = 0;
    for j in 0..n {
        let inv_col = intervals[j].col;
        while curr_col < inv_col {
            conn_line.push(' ');
            curr_col += 1;
        }
        conn_line.push('|');
        curr_col += 1;
    }
    lines.push(conn_line);

    let mut underline_line = String::new();
    let mut char_idx = 0;
    while char_idx < chars.len() {
        let mut in_interval = false;
        for inv in &intervals {
            if char_idx >= inv.start && char_idx < inv.end {
                in_interval = true;
                break;
            }
        }
        if in_interval {
            underline_line.push('_');
        } else {
            underline_line.push(' ');
        }
        char_idx += 1;
    }
    lines.push(underline_line);
    lines.push(buffer.to_string());

    lines.join("\n")
}

pub fn print_colorized(text: &str, spans: &[(usize, usize, &'static str)]) {
    let chars: Vec<char> = text.chars().collect();
    let mut output = String::new();
    let mut curr = 0;

    for &(start, end, tag) in spans {
        while curr < start && curr < chars.len() {
            output.push(chars[curr]);
            curr += 1;
        }

        let color = tag_to_color(tag);
        output.push_str(&format!("\x1b[38;5;{}m", color));
        while curr < end && curr < chars.len() {
            output.push(chars[curr]);
            curr += 1;
        }
        output.push_str("\x1b[0m");
    }

    while curr < chars.len() {
        output.push(chars[curr]);
        curr += 1;
    }

    println!("Colorized: {}", output);
}

pub fn spawn_zsh_session() -> (OsSession, tempfile::TempDir) {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string()),
    );
    let workspace_root = manifest_dir.parent().unwrap();
    let theme_path = manifest_dir.join("highlight/test_ansidecode_theme.toml");

    // Build the dynamic library target explicitly to ensure it exists
    oxizsh_test::build_module(workspace_root, Some("treeizsh-module"));

    let temp_dir = tempfile::tempdir().unwrap();
    let install_dir = temp_dir.path();

    // Read the compile-time target profile directory
    let target_profile_dir = oxizsh_test::target_profile_dir();

    // Run the manual installer script targeting the temp directory
    let install_script = workspace_root.join("scripts/manual-install.sh");
    let status = Command::new(oxizsh_test::zsh_binary())
        .args([
            "-c",
            format!(
                "source {} 2>&1 > {}/install.log",
                install_script.to_string_lossy(),
                install_dir.to_string_lossy()
            )
            .as_str(),
        ])
        .env("HOME", install_dir) // Set HOME to ZDOTDIR temp location
        .env("ZDOTDIR", install_dir)
        .env("INSTALL_DIR", install_dir)
        .env("SKIP_BUILD", "1")
        .env(
            "CARGO_BUILD_DIR",
            target_profile_dir
                .strip_prefix(workspace_root)
                .unwrap_or(&target_profile_dir),
        )
        .env("MODIFY_ZSHRC", "n") // Avoid interactive prompt
        .status()
        .expect("Failed to run manual installer script");
    assert!(
        status.success(),
        "manual-install.sh failed. check: {}/install.log",
        install_dir.to_string_lossy()
    );

    let integration_script = install_dir.join("treeizsh.zsh");

    let zshrc_content = format!(
        r#"
set -e
PROMPT='READY [%?] %% '
zmodload zsh/zle
source {:?}
typeset -g TREEIZSH_THEME={:?}
abort_command() {{ zle -I; BUFFER="" }}
zle -N abort_command; bindkey "^G" abort_command
set +e
"#,
        integration_script, theme_path
    );
    let zshrc_path = temp_dir.path().join(".zshrc");
    std::fs::write(&zshrc_path, zshrc_content).unwrap();

    let mut cmd = Command::new(oxizsh_test::zsh_binary());
    cmd.arg("-d");
    cmd.env("TERM", "xterm-256color");
    cmd.env("ZDOTDIR", temp_dir.path());
    cmd.current_dir(temp_dir.path());

    let mut session = Session::spawn(cmd).expect("Failed to spawn zsh");
    session.set_echo(false).expect("Failed to set echo");

    session
        .get_process_mut()
        .set_window_size(80, 24)
        .expect("Failed to set window size");

    let result = session.expect(PROMPT_RE);
    check_result(&mut session, result);

    (session, temp_dir)
}

struct CapturedWriter;

impl std::io::Write for CapturedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if std::env::var("TREEIZSH_LOG").is_ok() {
            let s = String::from_utf8_lossy(buf);
            print!("{}", s);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn highlight_buffer_in_mode(buffer: &str, mode: &str) -> String {
    let (session, _temp_dir) = spawn_zsh_session();
    let mut session = expectrl::session::log(session, CapturedWriter).unwrap();
    if mode != "zsh" {
        session
            .send_line(&format!("typeset -g TREEIZSH_MODE={:?}", mode))
            .unwrap();
        let result = session.expect(PROMPT_RE);
        check_result(&mut session, result);
    }
    if buffer.contains('\n') {
        session
            .send(&format!("\x1b[200~{}\x1b[201~", buffer))
            .unwrap();
    } else {
        session.send(buffer).unwrap();
    }
    session.send(b"\x0c").unwrap();
    let result = session.expect(PROMPT_RE);
    check_result(&mut session, result);
    session.send(b"\x07").unwrap();

    let result = session.expect(PROMPT_RE);
    let captures = check_result(&mut session, result);
    let captured = String::from_utf8_lossy(captures.before()).to_string();
    let _ = session.send_line("exit");
    captured
}

pub fn highlight_markup_in_mode(buffer: &str, mode: &str) -> String {
    let captured = highlight_buffer_in_mode(buffer, mode);
    parse_zle_highlight(&captured)
}

pub fn highlight_buffer(buffer: &str) -> String {
    highlight_buffer_in_mode(buffer, "zsh")
}

pub fn highlight_markup(buffer: &str) -> String {
    highlight_markup_in_mode(buffer, "zsh")
}

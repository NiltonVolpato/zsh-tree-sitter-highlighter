use expectrl::{Expect, Session, Regex};
use std::env;
use std::path::PathBuf;
use std::process::Command;
use expectrl::session::OsSession;
use expectrl::process::Termios;

pub const PROMPT: &str = "READY % ";

pub fn color_to_tag(color: u8) -> &'static str {
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

pub fn tag_to_color(tag: &str) -> u8 {
    match tag {
        "comment" => 17,
        "constant" => 18,
        "embedded" => 19,
        "function" => 20,
        "keyword" => 21,
        "number" => 22,
        "operator" => 23,
        "property" => 24,
        "punctuation.delimiter" => 25,
        "punctuation.special" => 26,
        "string" => 27,
        "string.escape" => 28,
        "text.emphasis" => 29,
        "text.literal" => 30,
        "text.reference" => 31,
        "text.strong" => 32,
        "text.title" => 33,
        "text.uri" => 34,
        "variable" => 35,
        "command.invalid" => 36,
        "path" => 37,
        "path.directory" => 38,
        _ => 15,
    }
}

pub fn parse_spans(stream: &str) -> (String, Vec<(usize, usize, &'static str)>) {
    let prompt = PROMPT;
    let idx = match stream.rfind(prompt) {
        Some(i) => i + prompt.len(),
        None => 0,
    };
    let content = &stream[idx..];

    let mut chars = content.chars().peekable();
    let mut current_color: Option<u8> = None;
    
    let mut clean_text = String::new();
    let mut spans = Vec::new();
    let mut current_span_start = 0;

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
                                        let char_count = clean_text.chars().count();
                                        if let Some(prev) = current_color {
                                            spans.push((current_span_start, char_count, color_to_tag(prev)));
                                        }
                                        current_color = Some(val);
                                        current_span_start = char_count;
                                    }
                                }
                            } else if seq == "39" || seq == "0" || seq.is_empty() {
                                if let Some(prev) = current_color {
                                    let char_count = clean_text.chars().count();
                                    spans.push((current_span_start, char_count, color_to_tag(prev)));
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
                clean_text.push(nc);
            }
        }
    }

    if let Some(prev) = current_color {
        let char_count = clean_text.chars().count();
        spans.push((current_span_start, char_count, color_to_tag(prev)));
    }

    let trimmed = clean_text.trim_end();
    let trimmed_len = trimmed.chars().count();
    let mut final_spans = Vec::new();
    for (start, end, tag) in spans {
        if start < trimmed_len {
            final_spans.push((start, end.min(trimmed_len), tag));
        }
    }

    (trimmed.to_string(), final_spans)
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
    result.trim().to_string()
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
        intervals.push(Interval { start, end, col, label });
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
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string()));
    let workspace_root = manifest_dir.parent().unwrap();
    let target_debug = workspace_root.join("target/debug");
    let theme_path = manifest_dir.join("highlight/test_ansidecode_theme.toml");
    let integration_script = workspace_root.join("zsh-ts-module/zsh-integration.zsh");

    let temp_dir = tempfile::tempdir().unwrap();
    let zshrc_content = format!(
        r#"
PROMPT='READY %% '
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
        target_debug, integration_script, theme_path
    );
    let zshrc_path = temp_dir.path().join(".zshrc");
    std::fs::write(&zshrc_path, zshrc_content).unwrap();

    let mut cmd = Command::new("zsh");
    cmd.arg("-d");
    cmd.env("TERM", "xterm-256color");
    cmd.env("ZDOTDIR", temp_dir.path());

    let mut session = Session::spawn(cmd).expect("Failed to spawn zsh");
    session.set_echo(false).expect("Failed to set echo");

    session
        .get_process_mut()
        .set_window_size(80, 24)
        .expect("Failed to set window size");

    session.expect(Regex(PROMPT)).unwrap();

    (session, temp_dir)
}

pub fn highlight_buffer(buffer: &str) -> String {
    let (mut session, _temp_dir) = spawn_zsh_session();
    session.send(buffer).unwrap();
    session.send(b"\x0c").unwrap();
    session.expect(Regex(PROMPT)).unwrap(); // Ignore all the terminal updates up to the prompt.
    session.send(b"\x07").unwrap();

    let captures = session.expect(Regex(PROMPT)).unwrap();
    let captured = String::from_utf8_lossy(captures.before()).to_string();
    let _ = session.send_line("exit");
    captured
}

pub fn highlight_markup(buffer: &str) -> String {
    let captured = highlight_buffer(buffer);
    parse_zle_highlight(&captured)
}

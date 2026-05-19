use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use zsh_tree_sitter_highlighter::api::{Request, Response};

fn exe_path() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_zsh-tree-sitter-highlighter")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .join("zsh-tree-sitter-highlighter")
        })
}

fn wait_for_socket(socket: &Path, timeout_ms: u64) -> bool {
    for _ in 0..timeout_ms / 10 {
        if socket.exists() {
            // Also verify we can actually connect
            if std::os::unix::net::UnixStream::connect(socket).is_ok() {
                return true;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

fn send_request(
    socket: &Path,
    ver: &str,
    lang: &str,
    pwd: &str,
    prebuffer: &str,
    buffer: &str,
) -> Vec<String> {
    let mut stream = UnixStream::connect(socket).expect("connect failed");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();

    // Send bencode request (raw bytes, no length prefix)
    let request = Request {
        version: ver.to_string(),
        mode: lang.to_string(),
        cwd: pwd.to_string(),
        prebuffer: prebuffer.to_string(),
        buffer: buffer.to_string(),
    };
    let request_bytes = bt_bencode::to_vec(&request).expect("serialize request");
    stream.write_all(&request_bytes).unwrap();
    let _ = stream.shutdown(std::net::Shutdown::Write);

    // Read response: byte_length + '\n' + exactly byte_length bytes
    let mut reader = BufReader::new(&stream);
    let mut length_line = String::new();
    reader
        .read_line(&mut length_line)
        .expect("read length line");
    let byte_length: usize = length_line.trim().parse().expect("parse byte length");

    let mut response_buf = vec![0u8; byte_length];
    reader
        .read_exact(&mut response_buf)
        .expect("read response bytes");

    let response: Response = bt_bencode::from_slice(&response_buf).expect("deserialize response");
    if response.regions.is_empty() {
        Vec::new()
    } else {
        response.regions.lines().map(|s| s.to_owned()).collect()
    }
}
fn start_daemon_in(dir: &std::path::Path) -> std::process::Child {
    let child = Command::new(exe_path())
        .args(["start-foreground"])
        .env("XDG_RUNTIME_DIR", dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start daemon");
    child
}

// ---------------------------------------------------------------------------
// Layer 2: zsh subprocess test infrastructure
// ---------------------------------------------------------------------------

struct DaemonGuard {
    child: Child,
    #[allow(dead_code)]
    dir: tempfile::TempDir,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Captured output from a zsh subprocess test.
struct ZshResult {
    /// Everything before the "---RESULT---" sentinel (includes ZLE mock output).
    output_before: String,
    /// The `typeset -p region_highlight` output (exactly one line).
    region_highlight: String,
    /// Combined stderr.
    stderr: String,
    /// Exit code of the zsh process.
    success: bool,
    /// The full script that was executed.
    script: String,
    /// Runtime dir used for this test (for normalizing paths in assertions).
    runtime_dir: Option<PathBuf>,
}

/// Start a daemon in a temp dir and return the guard + runtime dir path.
fn start_daemon() -> (DaemonGuard, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let child = start_daemon_in(dir.path());
    assert!(
        wait_for_socket(&socket, 3000),
        "daemon did not create socket in time"
    );
    let guard = DaemonGuard { child, dir };
    let runtime_dir = guard.dir.path().to_path_buf();
    (guard, runtime_dir)
}

/// Build the common zsh setup preamble: env vars, zle mock, source template.
fn zsh_setup_script(runtime_dir: &Path, extra_env: &str) -> String {
    let exe = exe_path();
    let template = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/zsh-integration.zsh");
    format!(
        r#"
export _ZSH_TS_HIGHLIGHTER_PATH='{exe}'
export _ZSH_TS_HIGHLIGHTER_RUNTIME_DIR='{runtime_dir}'
export _ZSH_TS_HIGHLIGHTER_VERSION=2
{extra_env}
# Mock zle so we can capture zle -M calls
zle() {{ echo "ZLE ${{(qq)@}}" }}
source '{template}'
"#,
        exe = exe.to_str().unwrap(),
        runtime_dir = runtime_dir.to_str().unwrap(),
        template = template.to_str().unwrap(),
    )
}

/// Run a zsh subprocess with a running daemon. The `test_body` should set
/// `BUFFER`, `PREBUFFER`, etc. and call `_zsh_ts_highlighter`.
fn run_zsh_with_daemon(daemon_dir: &Path, test_body: &str) -> ZshResult {
    run_zsh_with_daemon_extra(daemon_dir, "", test_body)
}

/// Like `run_zsh_with_daemon` but with extra env setup (e.g. unsetting vars).
fn run_zsh_with_daemon_extra(daemon_dir: &Path, extra_env: &str, test_body: &str) -> ZshResult {
    let script = format!(
        "{}\n{}\necho '---RESULT---'\ntypeset -p region_highlight\n",
        zsh_setup_script(daemon_dir, extra_env),
        test_body,
    );
    let mut result = run_zsh_script(&script);
    result.runtime_dir = Some(daemon_dir.to_path_buf());
    result
}

/// Run a zsh subprocess without a daemon (for error-path tests).
fn run_zsh_without_daemon(test_body: &str) -> ZshResult {
    let dir = tempfile::tempdir().unwrap();
    let runtime_dir = dir.path().to_path_buf();
    let mut result = run_zsh_with_daemon_extra(&runtime_dir, "", test_body);
    result.runtime_dir = Some(runtime_dir);
    result
}

/// Execute a zsh script and capture output.
fn run_zsh_script(script: &str) -> ZshResult {
    let result = Command::new("zsh")
        .arg("-c")
        .arg(script)
        .output()
        .expect("failed to run zsh");

    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr).into_owned();

    let (before, after) = stdout.split_once("---RESULT---\n").unwrap_or((&stdout, ""));

    ZshResult {
        output_before: before.to_owned(),
        region_highlight: after.trim_end().to_owned(),
        stderr,
        success: result.status.success(),
        script: script.to_owned(),
        runtime_dir: None,
    }
}

/// Comprehensive assertion helper for zsh subprocess tests.
///
/// IMPORTANT: This helper compares the **exact full output**, not substrings.
/// All callers must specify the complete expected `region_highlight` line and
/// the complete expected `output_before` content. This ensures tests catch
/// any unexpected extra output (e.g. spurious error messages, missing entries,
/// or extra ZLE calls) that substring matching would silently ignore.
fn assert_zsh_result(result: &ZshResult, expected_rh: &str, expected_output_before: &str) {
    let mut errors = Vec::new();

    if !result.success {
        errors.push(format!(
            "zsh exited with non-zero status\nstderr:\n{}",
            result.stderr
        ));
    }

    // Normalize variable paths (temp dirs) to "TMPDIR" so exact matching
    // works across different test runs without hardcoding paths.
    let (actual_before, expected_before) = if let Some(ref dir) = result.runtime_dir {
        let dir_str = dir.to_str().unwrap_or("");
        (
            result.output_before.replace(dir_str, "TMPDIR"),
            expected_output_before
                .replace("TMPDIR", dir_str)
                .replace(dir_str, "TMPDIR"),
        )
    } else {
        (
            result.output_before.clone(),
            expected_output_before.to_owned(),
        )
    };

    if actual_before != expected_before {
        errors.push(format!(
            "output_before mismatch.\nexpected:\n  {:?}\nactual:\n  {:?}",
            expected_before, actual_before
        ));
    }

    if result.region_highlight != expected_rh {
        errors.push(format!(
            "region_highlight mismatch.\nexpected:\n  {}\nactual:\n  {}",
            expected_rh, result.region_highlight
        ));
    }

    if !errors.is_empty() {
        panic!(
            "zsh test failed:\n{}\n\n--- full zsh script ---\n{}\n\n--- stdout ---\n{}---RESULT---\n{}\n\n--- stderr ---\n{}",
            errors.join("\n\n"),
            result.script,
            result.output_before,
            result.region_highlight,
            result.stderr
        );
    }
}

#[test]
fn test_daemon_highlight_zsh() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let mut child = start_daemon_in(dir.path());

    assert!(
        wait_for_socket(&socket, 3000),
        "daemon did not create socket in time"
    );

    let responses = send_request(&socket, "2", "zsh", "/tmp", "", "echo hello");
    assert!(!responses.is_empty(), "expected at least one span");
    let first = &responses[0];
    let parts: Vec<&str> = first.split_whitespace().collect();
    assert_eq!(parts.len(), 3, "expected 'start end style' format");
    assert_eq!(parts[0], "0");
    assert_eq!(parts[1], "4");

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_daemon_highlight_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let mut child = start_daemon_in(dir.path());

    assert!(
        wait_for_socket(&socket, 3000),
        "daemon did not create socket in time"
    );

    let responses = send_request(&socket, "2", "md", "/tmp", "", "# Hello");
    assert!(
        !responses.is_empty(),
        "expected at least one span for markdown heading"
    );
    let first = &responses[0];
    let parts: Vec<&str> = first.split_whitespace().collect();
    assert_eq!(parts.len(), 3);
    // Markdown heading: first span is the '#' marker
    assert_eq!(parts[0], "0");
    assert_eq!(parts[1], "1");

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_daemon_concurrent_connections() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let mut child = start_daemon_in(dir.path());

    assert!(wait_for_socket(&socket, 3000));

    let mut handles = Vec::new();
    for _ in 0..4 {
        let socket = socket.clone();
        let handle = thread::spawn(move || {
            let responses = send_request(&socket, "2", "zsh", "/tmp", "", "echo hello");
            assert!(!responses.is_empty());
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().unwrap();
    }

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_activate_renders_valid_zsh_script() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(exe_path())
        .args(["activate"])
        .env("XDG_RUNTIME_DIR", dir.path())
        .output()
        .expect("failed to run activate");

    assert!(output.status.success(), "activate command failed");
    let script = String::from_utf8(output.stdout).expect("invalid UTF-8");

    assert!(
        script.contains("_zsh_ts_highlighter"),
        "script missing _zsh_ts_highlighter function"
    );
    assert!(
        script.contains("add-zle-hook-widget"),
        "script missing add-zle-hook-widget call"
    );

    let mut child = Command::new("zsh")
        .args(["-n", "/dev/stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn zsh");

    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(script.as_bytes()).unwrap();
    }

    let output = child.wait_with_output().expect("failed to wait for zsh");
    assert!(
        output.status.success(),
        "zsh -n failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// End-to-end test: activate the highlighter in a real zsh with a PTY (via expect),
/// type a command, and verify that syntax highlighting ANSI escape codes appear.
#[test]
fn test_highlighting_works_in_zsh() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let mut daemon = start_daemon_in(dir.path());

    assert!(
        wait_for_socket(&socket, 3000),
        "daemon did not create socket in time"
    );

    let exe = exe_path();
    let runtime_dir_str = dir.path().to_str().unwrap();

    let expect_script = format!(
        r#"
set timeout 5
spawn zsh -f
expect "% "
send "export XDG_RUNTIME_DIR={runtime_dir_str}\r"
expect "% "
send "eval \"\$({exe} activate)\"\r"
expect "% "
send "echo -n foo\r"
expect "% "
send "exit\r"
expect eof
"#,
        exe = exe.to_str().unwrap(),
        runtime_dir_str = runtime_dir_str,
    );

    let result = Command::new("expect")
        .arg("-c")
        .arg(&expect_script)
        .output()
        .expect("failed to run expect");

    let _ = daemon.kill();
    let _ = daemon.wait();

    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);

    assert!(
        result.status.success(),
        "expect failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    // Syntax highlighting produces ANSI 24-bit color escapes: \x1b[38;2;...
    assert!(
        stdout.contains("\x1b[38;2;"),
        "expected ANSI color escapes in output (highlighting not working)\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// Regression test: multi-byte characters in buffer/prebuffer must be counted
/// by char, not by byte. If `send_request` regressed from `.chars().count()` to
/// `.len()`, the daemon would receive a wrong length and fail to parse the request.
#[test]
fn test_daemon_multibyte_buffer() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let mut child = start_daemon_in(dir.path());

    assert!(
        wait_for_socket(&socket, 3000),
        "daemon did not create socket in time"
    );

    // "café" = 4 chars, 5 bytes — if counted by .len() the daemon would get length=5
    // and read too many bytes, breaking the protocol
    let responses = send_request(&socket, "2", "zsh", "/tmp", "", "echo café");
    assert!(
        !responses.is_empty(),
        "expected at least one span for multibyte buffer"
    );

    let _ = child.kill();
    let _ = child.wait();
}

/// Regression test: multi-byte characters in the prebuffer must be counted by
/// char. The daemon shifts span offsets by `prebuffer.chars().count()`. If that
/// regressed to `.len()`, the offsets would be wrong for multi-byte prebuffer
/// content.
#[test]
fn test_daemon_multibyte_prebuffer() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let mut child = start_daemon_in(dir.path());

    assert!(
        wait_for_socket(&socket, 3000),
        "daemon did not create socket in time"
    );

    // prebuffer "café" = 4 chars (5 bytes), buffer "echo hello"
    let responses = send_request(&socket, "2", "zsh", "/tmp", "café", "echo hello");
    assert!(
        !responses.is_empty(),
        "expected at least one span with multibyte prebuffer"
    );
    // The spans should be relative to the buffer, not the full source.
    // "echo" in "echo hello" should start at offset 0 in the returned spans.
    let first = &responses[0];
    let parts: Vec<&str> = first.split_whitespace().collect();
    assert_eq!(
        parts[0], "0",
        "first span should start at 0 (buffer-relative)"
    );

    let _ = child.kill();
    let _ = child.wait();
}

/// Verify that an error message is shown when the daemon is not running.
/// Sources the integration script directly (skipping `activate` which would start a daemon).
#[test]
fn test_error_message_when_daemon_down() {
    let dir = tempfile::tempdir().unwrap();
    let exe = exe_path();
    let runtime_dir_str = dir.path().to_str().unwrap();
    let template_path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/zsh-integration.zsh");

    let expect_script = format!(
        r#"
set timeout 5
spawn zsh -f
expect "% "
send "export _ZSH_TS_HIGHLIGHTER_PATH={exe}\r"
expect "% "
send "export _ZSH_TS_HIGHLIGHTER_RUNTIME_DIR={runtime_dir_str}\r"
expect "% "
send "export _ZSH_TS_HIGHLIGHTER_VERSION=1\r"
expect "% "
send "source {template}\r"
expect "% "
send "echo -n foo\r"
expect "% "
send "exit\r"
expect eof
"#,
        exe = exe.to_str().unwrap(),
        runtime_dir_str = runtime_dir_str,
        template = template_path.to_str().unwrap(),
    );

    let result = Command::new("expect")
        .arg("-c")
        .arg(&expect_script)
        .output()
        .expect("failed to run expect");

    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);

    assert!(
        result.status.success(),
        "expect failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("daemon socket not found"),
        "expected 'daemon socket not found' error message in output\nstdout: {stdout}\nstderr: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Layer 2: zsh subprocess tests (exact output matching)
// ---------------------------------------------------------------------------

#[test]
fn test_zsh_echo_hello() {
    let (_guard, dir) = start_daemon();
    let result = run_zsh_with_daemon(
        &dir,
        r#"
region_highlight=()
BUFFER="echo hello"
PREBUFFER=""
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(
        &result,
        "typeset -a region_highlight=( '0 4 fg=#7aa2f7 memo=zsh_ts_highlighter' )",
        "",
    );
}

/// Markdown mode: plain text "Hello." has no markdown syntax, so the daemon
/// returns no spans. Verifies that ZSH_TS_HIGHLIGHTER_MODE is propagated through
/// the template to the daemon and that empty responses don't produce bogus entries.
#[test]
fn test_zsh_markdown_plain_text() {
    let (_guard, dir) = start_daemon();
    let result = run_zsh_with_daemon_extra(
        &dir,
        "export ZSH_TS_HIGHLIGHTER_MODE=md",
        r#"
region_highlight=()
BUFFER="Hello."
PREBUFFER=""
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(&result, "typeset -a region_highlight=(  )", "");
}

#[test]
fn test_zsh_echo_quoted_foo() {
    let (_guard, dir) = start_daemon();
    let result = run_zsh_with_daemon(
        &dir,
        r#"
region_highlight=()
BUFFER='echo "foo"'
PREBUFFER=""
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(
        &result,
        "typeset -a region_highlight=( '0 4 fg=#7aa2f7 memo=zsh_ts_highlighter' '5 10 fg=#e0af68 memo=zsh_ts_highlighter' )",
        "",
    );
}

#[test]
fn test_zsh_multibyte_buffer() {
    let (_guard, dir) = start_daemon();
    // echo "café" — the quoted string "café" is 6 chars (", c, a, f, é, ").
    // If char counting regressed to byte counting, é would be 2 bytes and the
    // span [5..11] for the string would be wrong.
    let result = run_zsh_with_daemon(
        &dir,
        r#"
region_highlight=()
BUFFER='echo "café"'
PREBUFFER=""
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(
        &result,
        "typeset -a region_highlight=( '0 4 fg=#7aa2f7 memo=zsh_ts_highlighter' '5 11 fg=#e0af68 memo=zsh_ts_highlighter' )",
        "",
    );
}

#[test]
fn test_zsh_multibyte_prebuffer() {
    let (_guard, dir) = start_daemon();
    // PREBUFFER="abcé\n" (5 chars, 6 bytes). BUFFER="echo hello".
    // Full source: "abcé\necho hello" → "echo" is parsed as a separate command.
    // The daemon shifts spans by prebuffer_char_len=5.
    // If char counting regressed to byte counting (6), the span would be off by 1.
    let result = run_zsh_with_daemon(
        &dir,
        r#"
region_highlight=()
BUFFER="echo hello"
PREBUFFER=$'abcé\n'
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(
        &result,
        "typeset -a region_highlight=( '0 4 fg=#7aa2f7 memo=zsh_ts_highlighter' )",
        "",
    );
}

#[test]
fn test_zsh_path_underline() {
    let (_guard, dir) = start_daemon();
    let result = run_zsh_with_daemon(
        &dir,
        r#"
region_highlight=()
BUFFER="cat /etc/passwd"
PREBUFFER=""
PWD="/"
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(
        &result,
        "typeset -a region_highlight=( '0 3 fg=#7aa2f7 memo=zsh_ts_highlighter' '4 15 underline memo=zsh_ts_highlighter' )",
        "",
    );
}

#[test]
fn test_zsh_empty_buffer() {
    let (_guard, dir) = start_daemon();
    let result = run_zsh_with_daemon(
        &dir,
        r#"
region_highlight=()
BUFFER=""
PREBUFFER=""
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(&result, "typeset -a region_highlight=(  )", "");
}

#[test]
fn test_zsh_memo_filtering_preserves_other() {
    let (_guard, dir) = start_daemon();
    let result = run_zsh_with_daemon(
        &dir,
        r#"
region_highlight=("0 10 some_other_plugin")
BUFFER="echo hello"
PREBUFFER=""
_zsh_ts_highlighter
"#,
    );
    // The other_plugin entry should be preserved; our memo entry is added.
    assert_zsh_result(
        &result,
        "typeset -a region_highlight=( '0 10 some_other_plugin' '0 4 fg=#7aa2f7 memo=zsh_ts_highlighter' )",
        "",
    );
}

#[test]
fn test_zsh_memo_filtering_removes_old_memo() {
    let (_guard, dir) = start_daemon();
    let result = run_zsh_with_daemon(
        &dir,
        r#"
region_highlight=("0 10 some_other_plugin" "0 4 fg=#7aa2f7 memo=zsh_ts_highlighter")
BUFFER="ls /tmp"
PREBUFFER=""
PWD="/"
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(
        &result,
        "typeset -a region_highlight=( '0 10 some_other_plugin' '0 2 fg=#7aa2f7 memo=zsh_ts_highlighter' '3 7 fg=#7aa2f7,underline memo=zsh_ts_highlighter' )",
        "",
    );
}

#[test]
fn test_zsh_daemon_socket_not_found() {
    let result = run_zsh_without_daemon(
        r#"
region_highlight=()
BUFFER="echo hello"
PREBUFFER=""
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(
        &result,
        "typeset -a region_highlight=(  )",
        "ZLE '-M' 'zsh-tree-sitter-highlighter: daemon socket not found at TMPDIR/daemon.sock'\n",
    );
}

#[test]
fn test_zsh_version_unset() {
    let (_guard, dir) = start_daemon();
    let result = run_zsh_with_daemon_extra(
        &dir,
        "unset _ZSH_TS_HIGHLIGHTER_VERSION",
        r#"
region_highlight=()
BUFFER="echo hello"
PREBUFFER=""
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(
        &result,
        "typeset -a region_highlight=(  )",
        "ZLE '-M' 'zsh-tree-sitter-highlighter: _ZSH_TS_HIGHLIGHTER_VERSION not set, activation may have failed'\n",
    );
}

/// When the client sends a newer protocol version than the daemon supports,
/// the daemon should return a clear error response instead of silently
/// closing the connection.
#[test]
fn test_zsh_version_mismatch() {
    let (_guard, dir) = start_daemon();
    let result = run_zsh_with_daemon_extra(
        &dir,
        r#"export _ZSH_TS_HIGHLIGHTER_VERSION=$(( $_ZSH_TS_HIGHLIGHTER_VERSION + 1 ))"#,
        r#"
region_highlight=()
BUFFER="echo hello"
PREBUFFER=""
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(
        &result,
        "typeset -a region_highlight=(  )",
        "ZLE '-M' 'zsh-tree-sitter-highlighter: version mismatch (client=3, daemon=2)'\n",
    );
}

/// Regression test: BUFFER ending with a newline must not break the protocol.
/// When print_kv sends a value that ends with '\n', print -r adds another '\n',
/// producing a double newline. The parser must consume the trailing '\n' so the
/// next record (EOM) is parsed correctly.
#[test]
fn test_zsh_buffer_ending_with_newline() {
    let (_guard, dir) = start_daemon();
    let result = run_zsh_with_daemon(
        &dir,
        r#"
region_highlight=()
BUFFER=$'echo hello\n'
PREBUFFER=""
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(
        &result,
        "typeset -a region_highlight=( '0 4 fg=#7aa2f7 memo=zsh_ts_highlighter' )",
        "",
    );
}

/// A zsh function defined in the current session should be highlighted as a
/// known command, not as an unknown command.
#[test]
fn test_zsh_function_highlighting() {
    let (_guard, dir) = start_daemon();
    let result = run_zsh_with_daemon(
        &dir,
        r#"
my-function() { echo "$@" }
region_highlight=()
BUFFER="my-function foo"
PREBUFFER=""
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(
        &result,
        "typeset -a region_highlight=( '0 11 fg=#7aa2f7 memo=zsh_ts_highlighter' )",
        "",
    );
}

/// A zsh alias defined in the current session should be highlighted as a
/// known command, not as an unknown command.
#[test]
fn test_zsh_alias_highlighting() {
    let (_guard, dir) = start_daemon();
    let result = run_zsh_with_daemon(
        &dir,
        r#"
alias my-alias='echo'
region_highlight=()
BUFFER="my-alias foo"
PREBUFFER=""
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(
        &result,
        "typeset -a region_highlight=( '0 8 fg=#7aa2f7 memo=zsh_ts_highlighter' )",
        "",
    );
}

/// An unknown command should get the red "unknown command" override.
#[test]
fn test_zsh_unknown_command() {
    let (_guard, dir) = start_daemon();
    let result = run_zsh_with_daemon(
        &dir,
        r#"
region_highlight=()
BUFFER="nonexistentcmd foo"
PREBUFFER=""
_zsh_ts_highlighter
"#,
    );
    assert_zsh_result(
        &result,
        "typeset -a region_highlight=( '0 14 fg=#7aa2f7 memo=zsh_ts_highlighter' '0 14 fg=#f7768e memo=zsh_ts_highlighter' )",
        "",
    );
}

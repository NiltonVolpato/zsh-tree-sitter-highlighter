use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

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
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

fn send_request(socket: &Path, ver: &str, lang: &str, pwd: &str, prebuffer: &str, buffer: &str) -> Vec<String> {
    let mut stream = UnixStream::connect(socket).expect("connect failed");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();

    // Send key=length:value records (character counts, matching zsh ${#var})
    let request = format!(
        "ver={}:{}\nlang={}:{}\npwd={}:{}\nprebuffer={}:{}\nbuffer={}:{}\nEOM=0:\n",
        ver.chars().count(), ver,
        lang.chars().count(), lang,
        pwd.chars().count(), pwd,
        prebuffer.chars().count(), prebuffer,
        buffer.chars().count(), buffer,
    );
    stream.write_all(request.as_bytes()).unwrap();

    let _ = stream.shutdown(std::net::Shutdown::Write);

    let reader = BufReader::new(&stream);
    reader
        .lines()
        .map(|l| l.unwrap())
        .filter(|l| !l.is_empty())
        .collect()
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

#[test]
fn test_daemon_highlight_zsh() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let mut child = start_daemon_in(dir.path());

    assert!(wait_for_socket(&socket, 3000), "daemon did not create socket in time");

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

    assert!(wait_for_socket(&socket, 3000), "daemon did not create socket in time");

    let responses = send_request(&socket, "2", "md", "/tmp", "", "# Hello");
    assert!(!responses.is_empty(), "expected at least one span for markdown heading");
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
    assert!(output.status.success(), "zsh -n failed: {}", String::from_utf8_lossy(&output.stderr));
}

/// End-to-end test: activate the highlighter in a real zsh with a PTY (via expect),
/// type a command, and verify that syntax highlighting ANSI escape codes appear.
#[test]
fn test_highlighting_works_in_zsh() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("daemon.sock");
    let mut daemon = start_daemon_in(dir.path());

    assert!(wait_for_socket(&socket, 3000), "daemon did not create socket in time");

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

/// Verify that an error message is shown when the daemon is not running.
/// Sources the integration script directly (skipping `activate` which would start a daemon).
#[test]
fn test_error_message_when_daemon_down() {
    let dir = tempfile::tempdir().unwrap();
    let exe = exe_path();
    let runtime_dir_str = dir.path().to_str().unwrap();
    let template_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("templates/zsh-integration.zsh");

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

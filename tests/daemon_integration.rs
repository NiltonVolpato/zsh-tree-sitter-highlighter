use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
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

fn wait_for_socket(socket: &PathBuf, timeout_ms: u64) -> bool {
    for _ in 0..timeout_ms / 10 {
        if socket.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

fn send_request(socket: &PathBuf, header: &str, lines: &[&str]) -> Vec<String> {
    let mut stream = UnixStream::connect(socket).expect("connect failed");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();

    stream
        .write_all(format!("{header}\n").as_bytes())
        .unwrap();
    for line in lines {
        stream.write_all(format!("{line}\n").as_bytes()).unwrap();
    }

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
        .args(["start"])
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

    let responses = send_request(&socket, "ver=1 lang=zsh lines=1", &["echo hello"]);
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

    let responses = send_request(&socket, "ver=1 lang=md lines=1", &["# Hello"]);
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
            let responses = send_request(&socket, "ver=1 lang=zsh lines=1", &["echo hello"]);
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

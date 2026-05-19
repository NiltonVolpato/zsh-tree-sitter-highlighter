use anyhow::{Context, Result, bail};
use rayon::ThreadPoolBuilder;
use std::{
    fs::{self, Permissions},
    io::{Write, stdout},
    os::{
        fd::AsRawFd,
        unix::{
            fs::PermissionsExt,
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    process,
    sync::Arc,
    time::Duration,
};

use crate::api::{Request, Response};
use crate::highlight::{HighlightEngine, LanguageConfig, Span};
use crate::theme::tokyonight_dark;
use serde::Deserialize;

const PROTOCOL_VERSION: &str = "2";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Parent,
    Child,
    Daemon,
}

const ACTIVATE_SCRIPT: &str = include_str!("../templates/zsh-integration.zsh");

fn pid_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("daemon.pid")
}

fn sock_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("daemon.sock")
}

fn read_pid(pid_file: &Path) -> Option<u32> {
    fs::read_to_string(pid_file).ok()?.trim().parse().ok()
}

fn pid_alive(pid: u32) -> bool {
    // SAFETY: kill(pid, 0) only checks if process exists, no signal is sent.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn handle_connection(mut stream: UnixStream, engine: Arc<HighlightEngine>) -> Result<()> {
    // Read the bencode request.  The client sends a raw bencode dictionary
    // (no length prefix) and keeps the connection open for the response.
    // We use the streaming deserializer directly and intentionally do NOT call
    // `deserializer.end()` — that would fail because the socket still has unread
    // bytes (the client is waiting for our response on the same stream).
    let mut deserializer = bt_bencode::Deserializer::from_reader(&stream);
    let request =
        Request::deserialize(&mut deserializer).context("unable to deserialize bencode request")?;

    // Verify protocol version
    if request.version != PROTOCOL_VERSION {
        return Ok(());
    }

    let language = match request.mode.as_str() {
        "markdown" | "md" => LanguageConfig::Markdown,
        _ => LanguageConfig::Zsh,
    };

    // Build full source: prebuffer + buffer
    let full_source = format!("{}{}", request.prebuffer, request.buffer);

    // Highlight with cwd for dynamic path resolution
    let pwd = Some(request.cwd.as_str());
    let spans = engine.highlight_with_pwd(language, &full_source, pwd)?;

    // Adjust span offsets: shift by prebuffer length, only return buffer spans
    let prebuffer_char_len = request.prebuffer.chars().count();
    let mut regions = Vec::new();
    for span in spans {
        // Skip spans entirely within prebuffer
        if span.end <= prebuffer_char_len {
            continue;
        }

        let start = if span.start < prebuffer_char_len {
            0
        } else {
            span.start - prebuffer_char_len
        };
        let end = span.end - prebuffer_char_len;

        if start >= end {
            continue;
        }

        regions.push(format!("{start} {end} {}", span.style));
    }

    let response = Response {
        regions: regions.join("\n"),
    };
    let response_bytes =
        bt_bencode::to_vec(&response).context("unable to serialize bencode response")?;

    // The zsh client expects: byte_length + '\n' + exactly byte_length bytes
    stream
        .write_all(format!("{}\n", response_bytes.len()).as_bytes())
        .context("unable to write response length")?;
    stream
        .write_all(&response_bytes)
        .context("unable to write response bytes")?;

    Ok(())
}

pub fn activate(runtime_dir: &Path) -> Result<()> {
    let (role, _already_running) = start_daemon_internal(runtime_dir, false)?;
    if role == Role::Parent {
        let exe = std::env::current_exe()?;
        let runtime_dir_str = runtime_dir.to_str().unwrap().trim_end_matches('/');
        let mut s = stdout().lock();
        writeln!(
            s,
            "export _ZSH_TS_HIGHLIGHTER_PATH={:?}",
            exe.to_str().unwrap()
        )?;
        writeln!(
            s,
            "export _ZSH_TS_HIGHLIGHTER_RUNTIME_DIR={:?}",
            runtime_dir_str
        )?;
        writeln!(
            s,
            "export _ZSH_TS_HIGHLIGHTER_VERSION={:?}",
            PROTOCOL_VERSION
        )?;
        s.write_all(ACTIVATE_SCRIPT.as_bytes())?;
        s.flush()?;
    }
    Ok(())
}

pub fn start_daemon(runtime_dir: &Path) -> Result<()> {
    start_daemon_internal(runtime_dir, false)?;
    Ok(())
}

pub fn start_daemon_foreground(runtime_dir: &Path) -> Result<()> {
    start_daemon_internal(runtime_dir, true)?;
    Ok(())
}

pub fn restart_daemon(runtime_dir: &Path) {
    stop_daemon(runtime_dir);
    let _ = start_daemon(runtime_dir);
}

pub fn stop_daemon(runtime_dir: &Path) {
    let pid_file = pid_path(runtime_dir);
    if let Some(pid) = read_pid(&pid_file)
        && pid_alive(pid)
    {
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        let _ = fs::remove_file(&pid_file);
        let _ = fs::remove_file(sock_path(runtime_dir));
    }
}

pub fn status_daemon(runtime_dir: &Path) -> Result<()> {
    let pid_file = pid_path(runtime_dir);
    if let Some(pid) = read_pid(&pid_file)
        && pid_alive(pid)
    {
        println!("Daemon is running. PID {pid}.");
        Ok(())
    } else {
        bail!("Daemon is stopped.");
    }
}

fn start_daemon_internal(runtime_dir: &Path, no_daemon: bool) -> Result<(Role, bool)> {
    let pid_file = pid_path(runtime_dir);

    if let Some(pid) = read_pid(&pid_file)
        && pid_alive(pid)
    {
        return Ok((Role::Parent, true));
    }

    fs::create_dir_all(runtime_dir).context("unable to create runtime directory")?;

    if !no_daemon {
        // Double-fork to daemonize
        match unsafe { libc::fork() } {
            -1 => bail!("fork #1 failed"),
            0 => {}
            _ => return Ok((Role::Parent, false)),
        }

        unsafe { libc::setsid() };

        match unsafe { libc::fork() } {
            -1 => bail!("fork #2 failed"),
            0 => {}
            _ => return Ok((Role::Child, false)),
        }

        // Redirect stdio to /dev/null
        unsafe {
            let devnull = std::fs::File::open("/dev/null").unwrap();
            libc::dup2(devnull.as_raw_fd(), libc::STDIN_FILENO);
            libc::dup2(devnull.as_raw_fd(), libc::STDOUT_FILENO);
            libc::dup2(devnull.as_raw_fd(), libc::STDERR_FILENO);
        }
    }

    let my_pid = process::id();
    fs::write(&pid_file, format!("{my_pid}\n"))
        .with_context(|| format!("unable to write PID file {pid_file:?}"))?;
    fs::set_permissions(&pid_file, Permissions::from_mode(0o1600))
        .with_context(|| format!("unable to set permissions of {pid_file:?}"))?;

    let socket_path = sock_path(runtime_dir);
    let _ = fs::remove_file(&socket_path);

    let pool = ThreadPoolBuilder::new().num_threads(4).build().unwrap();

    let theme = tokyonight_dark();
    let engine = Arc::new(HighlightEngine::new(theme)?);

    // Warm-up highlighting
    let init_engine = Arc::clone(&engine);
    pool.spawn(move || {
        let _ = init_engine.highlight(LanguageConfig::Zsh, "echo hello");
    });

    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("unable to bind socket {socket_path:?}"))?;
    fs::set_permissions(&socket_path, Permissions::from_mode(0o1600))
        .with_context(|| format!("unable to set permissions of {socket_path:?}"))?;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let engine = Arc::clone(&engine);
                pool.spawn(move || {
                    let _ = handle_connection(stream, engine);
                });
            }
            Err(_) => {
                break;
            }
        }
    }

    let _ = fs::remove_file(&pid_file);
    let _ = fs::remove_file(&socket_path);

    Ok((Role::Daemon, false))
}

/// One-shot highlight for CLI usage.
pub fn highlight_one_shot(lang: LanguageConfig, text: &str) -> Result<Vec<Span>> {
    let theme = tokyonight_dark();
    let engine = HighlightEngine::new(theme)?;
    engine.highlight(lang, text)
}

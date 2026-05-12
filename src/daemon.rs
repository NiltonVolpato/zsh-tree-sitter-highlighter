use anyhow::{Context, Result, bail};
use rayon::ThreadPoolBuilder;
use std::{
    fs::{self, Permissions},
    io::{BufRead, BufReader, Write, stdout},
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

use crate::highlight::{HighlightEngine, LanguageConfig, Span};
use crate::theme::tokyonight_dark;

const PROTOCOL_VERSION: &str = "1";

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
    let mut reader = BufReader::new(&stream);

    // read header line
    let mut header = String::new();
    reader
        .read_line(&mut header)
        .context("unable to read header")?;

    let mut client_version = None;
    let mut lang = "zsh";
    let mut lines_count = 0usize;

    for part in header.split_ascii_whitespace() {
        if let Some((key, value)) = part.split_once('=') {
            match key {
                "ver" => client_version = Some(value),
                "lang" => lang = value,
                "lines" => {
                    lines_count = value.parse().context("unable to parse lines count")?;
                }
                _ => {}
            }
        }
    }

    // verify protocol version
    if client_version.is_none_or(|v| v != PROTOCOL_VERSION) {
        return Ok(());
    }

    // read exactly lines_count lines
    let mut text = String::new();
    for _ in 0..lines_count {
        let mut line = String::new();
        reader.read_line(&mut line).context("unable to read line")?;
        text.push_str(&line);
    }

    // trim trailing newline that we added for protocol framing
    while text.ends_with('\n') {
        text.pop();
    }

    let language = match lang {
        "markdown" | "md" => LanguageConfig::Markdown,
        _ => LanguageConfig::Zsh,
    };

    let spans = engine.highlight(language, &text)?;

    for span in spans {
        stream
            .write_all(format!("{} {} {}\n", span.start, span.end, span.style).as_bytes())
            .context("unable to write response")?;
    }

    Ok(())
}

pub fn activate(runtime_dir: &Path) -> Result<()> {
    let (role, _already_running) = start_daemon_internal(runtime_dir, false)?;
    if role == Role::Parent {
        let exe = std::env::current_exe()?;
        let runtime_dir_str = runtime_dir
            .to_str()
            .unwrap()
            .trim_end_matches('/');
        let mut s = stdout().lock();
        writeln!(s, "export _ZSH_TS_HIGHLIGHTER_PATH={:?}", exe.to_str().unwrap())?;
        writeln!(s, "export _ZSH_TS_HIGHLIGHTER_RUNTIME_DIR={:?}", runtime_dir_str)?;
        writeln!(s, "export _ZSH_TS_HIGHLIGHTER_VERSION={:?}", PROTOCOL_VERSION)?;
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

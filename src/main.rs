mod api;
mod daemon;
mod dynamic;
mod highlight;
mod protocol;
mod theme;

use anyhow::{Context, Result, bail};
use daemon::{
    activate, highlight_one_shot, restart_daemon, start_daemon, start_daemon_foreground,
    status_daemon, stop_daemon,
};
use highlight::LanguageConfig;
use std::io::Read;
use std::path::PathBuf;

fn runtime_dir() -> Result<PathBuf> {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".local/share/zsh-tree-sitter-highlighter"))
        })
        .context("unable to determine runtime directory")?;
    Ok(dir)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        bail!(
            "Usage: {} <activate|start|start-foreground|stop|restart|status|highlight> [args...]",
            args[0]
        );
    }

    match args[1].as_str() {
        "activate" => {
            let dir = runtime_dir()?;
            activate(&dir)?;
        }
        "start" => {
            let dir = runtime_dir()?;
            start_daemon(&dir)?;
            println!("Daemon started.");
        }
        "start-foreground" => {
            let dir = runtime_dir()?;
            start_daemon_foreground(&dir)?;
        }
        "stop" => {
            let dir = runtime_dir()?;
            stop_daemon(&dir);
            println!("Daemon stopped.");
        }
        "restart" => {
            let dir = runtime_dir()?;
            restart_daemon(&dir);
            println!("Daemon restarted.");
        }
        "status" => {
            let dir = runtime_dir()?;
            status_daemon(&dir)?;
        }
        "highlight" => {
            let lang = if args.len() >= 4 && args[2] == "--lang" {
                match args[3].as_str() {
                    "markdown" | "md" => LanguageConfig::Markdown,
                    "zsh" => LanguageConfig::Zsh,
                    other => bail!("unknown language: {other}"),
                }
            } else {
                LanguageConfig::Zsh
            };
            let text = if args.len() >= 4 && args[2] == "--lang" {
                if args.len() >= 5 {
                    args[4..].join(" ")
                } else {
                    let mut buf = String::new();
                    std::io::stdin().read_to_string(&mut buf)?;
                    buf
                }
            } else if args.len() >= 3 {
                args[2..].join(" ")
            } else {
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf)?;
                buf
            };
            let spans = highlight_one_shot(lang, &text)?;
            for span in spans {
                println!("{} {} {}", span.start, span.end, span.style);
            }
        }
        _ => {
            bail!("Unknown command: {}", args[1]);
        }
    }

    Ok(())
}

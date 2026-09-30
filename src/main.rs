use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use dispositif::config::{Config, expand_tilde};
use dispositif::state::StateDir;

/// Answer allowlisted Telegram messages with headless Claude Code runs.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Poll forever and answer every admitted message with a claude run.
    Run(ConfigArg),
    /// Print one JSON line per admitted message; run nothing.
    Watch {
        #[command(flatten)]
        config: ConfigArg,
        /// One poll cycle, then exit.
        #[arg(long)]
        once: bool,
    },
    /// Validate the config and print its rules and tiers.
    Check(ConfigArg),
}

#[derive(Args)]
struct ConfigArg {
    /// Path to the TOML config.
    #[arg(long, env = "DISPOSITIF_CONFIG")]
    config: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let res = match cli.cmd {
        Cmd::Run(c) => Config::load(&c.config)
            .and_then(|cfg| dispositif::runner::run(&cfg, &StateDir::from_env())),
        Cmd::Watch { config, once } => Config::load(&config.config)
            .and_then(|cfg| dispositif::poll::watch(&cfg, &StateDir::from_env(), once)),
        Cmd::Check(c) => Config::load(&c.config).map(|cfg| print_check(&c.config, &cfg)),
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn print_check(path: &std::path::Path, cfg: &Config) {
    println!("config ok: {}", path.display());
    println!("agent: {} @{}", cfg.agent_id, cfg.agent_username);
    println!("mcp: {} every {}s", cfg.mcp_url, cfg.interval_secs);
    println!("rules (first match wins):");
    for r in &cfg.rules {
        let from: Vec<String> = r.from.iter().map(i64::to_string).collect();
        println!(
            "  {:<16} peer={:<16} from=[{}] sender={} trigger={} trust={}{}",
            r.name,
            r.peer,
            from.join(","),
            r.sender.as_str(),
            r.trigger.as_str(),
            r.trust,
            if r.min_chars > 0 {
                format!(" min_chars={}", r.min_chars)
            } else {
                String::new()
            }
        );
    }
    println!("tiers:");
    for (name, t) in &cfg.tiers {
        let mut line = format!("  {name:<16} cwd={}", t.cwd);
        if t.restricted {
            line += &format!(" restricted tools={}", t.tools.as_deref().unwrap_or(""));
        }
        if let Some(mode) = &t.permission_mode {
            line += &format!(" permission_mode={mode}");
        }
        if let Some(dm) = &t.dm_peer {
            line += &format!(" dm_peer={dm}");
        }
        if !cfg.rules.iter().any(|r| r.trust == *name) {
            line += " (no rule uses it)";
        }
        if !expand_tilde(&t.cwd).is_dir() {
            line += " (warning: cwd does not exist here)";
        }
        println!("{line}");
    }
}

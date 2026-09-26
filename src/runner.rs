//! Answer each admitted event with one headless `claude -p` run.
//!
//! The trust tier of the admitting rule decides the run's capabilities. The model
//! never sends the reply: its final text is posted by this runner into the chat the
//! event came from, so no tier can write to any other chat.

use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::json;
use wait_timeout::ChildExt;

use crate::config::{Config, Tier, expand_tilde};
use crate::mcp::Mcp;
use crate::poll::{Event, poll_cycle};
use crate::state::{Session, Sessions, StateDir};
use crate::tg::{Message, MessageList};
use crate::{log, truncate_chars};

/// Telegram caps a message at 4096 characters; stay clear of it.
const REPLY_MAX_CHARS: usize = 4000;

/// `dispositif run`: poll forever, answer every admitted event.
pub fn run(cfg: &Config, dir: &StateDir) -> Result<()> {
    let started = crate::now();
    let mut state = dir.load_state()?;
    let mut sessions = dir.load_sessions()?;
    let mut mcp = Mcp::new(&cfg.mcp_url);
    let mut failing = false;
    log("runner started");
    loop {
        let mut events = Vec::new();
        // Events collected before a mid-poll failure are still answered: `last`
        // has moved past them, so they will not come back.
        match poll_cycle(&mut mcp, cfg, dir, &mut state, started, &mut |ev| {
            events.push(ev)
        }) {
            Ok(()) if failing => {
                log("poll recovered");
                failing = false;
            }
            Ok(()) => {}
            Err(e) if !failing => {
                log(&format!("poll failed: {e:#}"));
                failing = true;
            }
            Err(_) => {}
        }
        for ev in &events {
            log(&format!(
                "event peer={} msg={} rule={}",
                ev.peer, ev.id, ev.rule
            ));
            if let Err(e) = handle(cfg, dir, &mut sessions, ev) {
                log(&format!(
                    "handle failed peer={} msg={}: {e:#}",
                    ev.peer, ev.id
                ));
            }
        }
        thread::sleep(Duration::from_secs(cfg.interval_secs));
    }
}

/// Run `f` inside a fresh MCP session that is always closed afterwards.
fn with_session<T>(url: &str, f: impl FnOnce(&mut Mcp) -> Result<T>) -> Result<T> {
    let mut mcp = Mcp::new(url);
    let res = mcp.connect().and_then(|()| f(&mut mcp));
    mcp.close();
    res
}

/// Answer one event: show typing, gather context, run claude, post the reply.
/// A failed run still gets the configured fallback reply; an empty result gets none.
pub fn handle(cfg: &Config, dir: &StateDir, sessions: &mut Sessions, ev: &Event) -> Result<()> {
    let ctx = with_session(&cfg.mcp_url, |mcp| {
        mcp.call("tg_typing_send", json!({"peer": ev.peer}))?;
        let got: MessageList = mcp.call_as(
            "tg_messages_list",
            json!({"peer": ev.peer, "limit": cfg.history, "format": "json"}),
        )?;
        Ok(format_history(got.messages, ev.id))
    })?;
    let reply = match run_event(cfg, dir, sessions, ev, &ctx) {
        Ok(r) => r,
        Err(e) => {
            log(&format!("run failed peer={} msg={}: {e:#}", ev.peer, ev.id));
            cfg.fallback_reply.trim().to_owned()
        }
    };
    if reply.is_empty() {
        return Ok(());
    }
    with_session(&cfg.mcp_url, |mcp| {
        mcp.call(
            "tg_messages_send",
            json!({
                "peer": ev.peer,
                "text": truncate_chars(&reply, REPLY_MAX_CHARS),
                "parseMode": "plain",
                "replyTo": ev.id,
            }),
        )
    })?;
    Ok(())
}

/// Context lines `[id] name (reply to N): text`, oldest first, up to `upto`.
pub fn format_history(mut msgs: Vec<Message>, upto: i64) -> String {
    msgs.sort_by_key(|m| m.id);
    msgs.iter()
        .filter(|m| m.id <= upto)
        .map(|m| {
            let who = match (m.from_name.as_deref(), m.from_id) {
                (Some(name), _) if !name.is_empty() => name.to_owned(),
                (_, Some(id)) => id.to_string(),
                _ => "?".to_owned(),
            };
            let body = match m.text() {
                "" => format!("[{}]", m.kind.as_deref().unwrap_or("message")),
                t => t.to_owned(),
            };
            let re = m
                .reply_to_id()
                .map(|id| format!(" (reply to {id})"))
                .unwrap_or_default();
            format!("[{}] {who}{re}: {body}", m.id)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn build_prompt(cfg: &Config, tier: &Tier, ev: &Event, ctx: &str) -> String {
    let from = ev
        .from
        .clone()
        .or_else(|| ev.from_id.map(|id| id.to_string()))
        .unwrap_or_else(|| "?".into());
    [
        cfg.preamble.trim().to_owned(),
        tier.instructions.trim().to_owned(),
        format!(
            "Chat: {} (peer {}). Recent messages:\n{ctx}",
            ev.chat, ev.peer
        ),
        format!("Answer this message [{}] from {from}:\n{}", ev.id, ev.text),
    ]
    .join("\n\n")
}

/// The claude invocation for a tier. A restricted tier gets no shell, no MCP
/// servers and only its listed tools, and is never given bypassPermissions.
pub fn build_command(cfg: &Config, tier: &Tier, resume: Option<&str>) -> Command {
    let mut cmd = Command::new(expand_tilde(&cfg.claude_bin));
    cmd.args(["-p", "--output-format", "json"]);
    if tier.restricted {
        // Rejoined with bare commas, so claude reads exactly the names `check` saw.
        let tools: Vec<&str> = tier
            .tools
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect();
        cmd.args([
            "--restricted",
            "--strict-mcp-config",
            "--tools",
            &tools.join(","),
        ]);
    }
    if let Some(mode) = &tier.permission_mode
        && !(tier.restricted && mode == "bypassPermissions")
    {
        cmd.args(["--permission-mode", mode]);
    }
    if let Some(id) = resume {
        cmd.args(["--resume", id]);
    }
    cmd.current_dir(expand_tilde(&tier.cwd));
    for (k, v) in &cfg.claude_env {
        cmd.env(k, expand_tilde(v));
    }
    cmd
}

/// The session to resume for `key`, if one exists and is younger than `ttl_secs`.
pub fn fresh_session<'a>(
    sessions: &'a Sessions,
    key: &str,
    ttl_secs: u64,
    now: f64,
) -> Option<&'a str> {
    sessions
        .get(key)
        .filter(|s| now - s.at < ttl_secs as f64)
        .map(|s| s.id.as_str())
}

#[derive(Debug, Deserialize)]
pub struct ClaudeOutput {
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    #[serde(default)]
    pub num_turns: Option<u64>,
    #[serde(default)]
    pub is_error: bool,
}

fn run_event(
    cfg: &Config,
    dir: &StateDir,
    sessions: &mut Sessions,
    ev: &Event,
    ctx: &str,
) -> Result<String> {
    let tier = cfg
        .tiers
        .get(&ev.trust)
        .ok_or_else(|| anyhow!("no tier {:?}", ev.trust))?;
    let key = format!("{}:{}", ev.peer, ev.trust);
    let resume =
        fresh_session(sessions, &key, cfg.session_ttl_secs, crate::now()).map(str::to_owned);
    let prompt = build_prompt(cfg, tier, ev, ctx);
    let cmd = build_command(cfg, tier, resume.as_deref());
    let out = run_claude(cmd, prompt, Duration::from_secs(cfg.run_timeout_secs))?;
    if out.is_error {
        let msg = out.result.unwrap_or_default();
        bail!("claude error: {}", truncate_chars(&msg, 300));
    }
    let session_id = out
        .session_id
        .ok_or_else(|| anyhow!("claude output has no session_id"))?;
    sessions.insert(
        key,
        Session {
            id: session_id,
            at: crate::now(),
        },
    );
    // The reply is ready; losing the session only costs the thread its memory.
    if let Err(e) = dir.save_sessions(sessions) {
        log(&format!("saving sessions failed: {e:#}"));
    }
    log(&format!(
        "run ok peer={} msg={} trust={} cost={} turns={}",
        ev.peer,
        ev.id,
        ev.trust,
        out.total_cost_usd.map_or("?".into(), |c| c.to_string()),
        out.num_turns.map_or("?".into(), |n| n.to_string()),
    ));
    Ok(out.result.unwrap_or_default().trim().to_owned())
}

/// How long output may take to close once the run's process group is gone.
const READ_GRACE: Duration = Duration::from_secs(5);

/// Run claude with `prompt` on stdin in its own process group. After `timeout`,
/// or as soon as claude exits, the whole group is killed: nothing a run
/// started outlives it or keeps its output open.
pub fn run_claude(mut cmd: Command, prompt: String, timeout: Duration) -> Result<ClaudeOutput> {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = cmd
        .spawn()
        .with_context(|| format!("starting {}", cmd.get_program().to_string_lossy()))?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    // A child that exits without reading stdin makes this fail with a broken pipe;
    // its exit status tells the real story, so the write result is not checked.
    thread::spawn(move || stdin.write_all(prompt.as_bytes()));
    let stdout = drain(child.stdout.take().expect("stdout is piped"));
    let stderr = drain(child.stderr.take().expect("stderr is piped"));
    let waited = child.wait_timeout(timeout);
    kill_group(child.id());
    let Some(status) = waited? else {
        let _ = child.wait();
        bail!("claude timed out after {}s", timeout.as_secs());
    };
    let stdout = stdout
        .recv_timeout(READ_GRACE)
        .map_err(|_| anyhow!("claude exited but its output stayed open"))?;
    let stderr = stderr.recv_timeout(READ_GRACE).unwrap_or_default();
    if !status.success() {
        let err = String::from_utf8_lossy(&stderr);
        let err = err.trim();
        let tail_start = err.chars().count().saturating_sub(300);
        let tail: String = err.chars().skip(tail_start).collect();
        bail!("claude {status}: {tail}");
    }
    serde_json::from_slice(&stdout).context("claude output is not the expected JSON")
}

fn kill_group(pgid: u32) {
    // ESRCH (the group is already empty) is the common case and needs no report.
    if let Ok(pgid) = libc::pid_t::try_from(pgid) {
        // SAFETY: killpg only sends a signal; pgid is the group spawn created.
        unsafe { libc::killpg(pgid, libc::SIGKILL) };
    }
}

fn drain(mut r: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = r.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_lines() {
        let msgs: Vec<Message> = serde_json::from_value(json!([
            {"id": 12, "fromId": 7, "text": "later"},
            {"id": 10, "fromName": "Ann", "fromId": 5, "text": "hi"},
            {"id": 11, "fromName": "Bob", "type": "photo", "replyTo": {"messageId": 10}},
        ]))
        .unwrap();
        assert_eq!(
            format_history(msgs, 11),
            "[10] Ann: hi\n[11] Bob (reply to 10): [photo]"
        );
    }

    #[test]
    fn session_ttl() {
        let mut s = Sessions::new();
        s.insert(
            "p:full".into(),
            Session {
                id: "abc".into(),
                at: 1000.0,
            },
        );
        assert_eq!(fresh_session(&s, "p:full", 100, 1050.0), Some("abc"));
        assert_eq!(fresh_session(&s, "p:full", 100, 1100.0), None);
        assert_eq!(fresh_session(&s, "p:other", 100, 1050.0), None);
    }
}

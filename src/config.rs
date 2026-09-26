//! Policy: who gets through (rules) and what a run may do (tiers).
//! Everything owner-specific lives in the config file, never in code.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_mcp_url")]
    pub mcp_url: String,
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
    pub agent_id: i64,
    pub agent_username: String,
    #[serde(default = "default_claude_bin")]
    pub claude_bin: String,
    #[serde(default)]
    pub claude_env: BTreeMap<String, String>,
    #[serde(default = "default_session_ttl")]
    pub session_ttl_secs: u64,
    #[serde(default = "default_run_timeout")]
    pub run_timeout_secs: u64,
    #[serde(default = "default_history")]
    pub history: u32,
    pub preamble: String,
    pub fallback_reply: String,
    #[serde(rename = "rule", default)]
    pub rules: Vec<Rule>,
    #[serde(rename = "tier", default)]
    pub tiers: BTreeMap<String, Tier>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    /// Dialog peer as mcp-tg prints it (bot-API style numeric id), or "*".
    pub peer: String,
    pub from: Vec<i64>,
    pub trigger: Trigger,
    pub trust: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Any,
    MentionOrReply,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Any => "any",
            Trigger::MentionOrReply => "mention_or_reply",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tier {
    pub cwd: String,
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub restricted: bool,
    #[serde(default)]
    pub tools: Option<String>,
    pub instructions: String,
}

fn default_mcp_url() -> String {
    "http://127.0.0.1:8788".into()
}
fn default_interval() -> u64 {
    10
}
fn default_claude_bin() -> String {
    "claude".into()
}
fn default_session_ttl() -> u64 {
    86_400
}
fn default_run_timeout() -> u64 {
    900
}
fn default_history() -> u32 {
    15
}

impl Config {
    /// Everyone who can admit a message through some rule, plus the agent itself.
    pub fn allowlisted_senders(&self) -> std::collections::HashSet<i64> {
        self.rules
            .iter()
            .flat_map(|r| r.from.iter().copied())
            .chain(std::iter::once(self.agent_id))
            .collect()
    }

    pub fn load(path: &Path) -> Result<Config> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        Config::parse(&raw).with_context(|| format!("config {}", path.display()))
    }

    pub fn parse(raw: &str) -> Result<Config> {
        let cfg: Config = toml::from_str(raw)?;
        let problems = cfg.problems();
        if !problems.is_empty() {
            bail!("invalid config:\n  {}", problems.join("\n  "));
        }
        Ok(cfg)
    }

    /// Every reason the config cannot be trusted, so `check` reports them all at once.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.mcp_url.starts_with("http://") {
            out.push(format!(
                "mcp_url {:?} must be http://: mcp-tg serves plain HTTP locally and this build has no TLS",
                self.mcp_url
            ));
        }
        if self.agent_id == 0 {
            out.push("agent_id must be a non-zero Telegram user id".into());
        }
        let name = &self.agent_username;
        if name.is_empty() || name.starts_with('@') || name.contains(char::is_whitespace) {
            out.push(format!(
                "agent_username {name:?} must be a bare username without '@'"
            ));
        }
        if self.interval_secs == 0 {
            out.push("interval_secs must be positive".into());
        }
        if self.run_timeout_secs == 0 {
            out.push("run_timeout_secs must be positive".into());
        }
        if self.fallback_reply.trim().is_empty() {
            out.push("fallback_reply is empty: a failed run would go unanswered".into());
        }
        if self.rules.is_empty() {
            out.push("no [[rule]]: nothing would ever get through".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for r in &self.rules {
            let at = format!("rule {:?}", r.name);
            if r.name.is_empty() {
                out.push("a rule has an empty name".into());
            } else if !seen.insert(r.name.as_str()) {
                out.push(format!("{at}: duplicate rule name"));
            }
            if !valid_peer(&r.peer) {
                out.push(format!(
                    "{at}: peer {:?} must be \"*\" or a numeric dialog id",
                    r.peer
                ));
            }
            if r.from.is_empty() {
                out.push(format!("{at}: from is empty, the rule would admit no one"));
            }
            if r.from.iter().any(|&id| id <= 0) {
                out.push(format!(
                    "{at}: from must hold user ids, which are positive; chat ids go in peer"
                ));
            }
            if r.from.contains(&self.agent_id) {
                out.push(format!(
                    "{at}: from contains agent_id; the agent's own messages never match"
                ));
            }
            if !self.tiers.contains_key(&r.trust) {
                out.push(format!(
                    "{at}: trust {:?} names no [tier.{}]",
                    r.trust, r.trust
                ));
            }
        }
        for (name, t) in &self.tiers {
            let at = format!("tier {name:?}");
            if t.cwd.trim().is_empty() {
                out.push(format!("{at}: cwd is empty"));
            }
            if let Some(mode) = &t.permission_mode
                && !PERMISSION_MODES.contains(&mode.as_str())
            {
                out.push(format!(
                    "{at}: permission_mode {mode:?} is not one of {PERMISSION_MODES:?}"
                ));
            }
            if !t.restricted && t.tools.is_some() {
                out.push(format!(
                    "{at}: tools applies only with restricted = true; without it every tool is available"
                ));
            }
            if t.restricted {
                if t.tools.as_deref().is_none_or(|s| s.trim().is_empty()) {
                    out.push(format!(
                        "{at}: restricted tier needs a non-empty tools list"
                    ));
                }
                let refused: Vec<&str> = t
                    .tools
                    .as_deref()
                    .unwrap_or("")
                    .split(',')
                    .map(str::trim)
                    .filter(|tool| !tool.is_empty() && !RESTRICTED_TOOLS.contains(tool))
                    .collect();
                if !refused.is_empty() {
                    out.push(format!(
                        "{at}: restricted tier may list only {RESTRICTED_TOOLS:?}, not {refused:?}"
                    ));
                }
                if t.permission_mode.as_deref() == Some("bypassPermissions") {
                    out.push(format!(
                        "{at}: restricted tier cannot use permission_mode bypassPermissions"
                    ));
                }
            }
        }
        out
    }
}

/// Accepted by `claude --permission-mode` (2.1.282); "default" is an alias.
const PERMISSION_MODES: [&str; 7] = [
    "acceptEdits",
    "auto",
    "bypassPermissions",
    "default",
    "dontAsk",
    "manual",
    "plan",
];

/// What a restricted tier may name in `--tools`: tools that neither run code nor
/// write files. An allowlist, because claude keeps adding code-running tools
/// (Monitor) and parses `--tools` on spaces as well as commas.
pub const RESTRICTED_TOOLS: [&str; 5] = ["Read", "Grep", "Glob", "WebFetch", "WebSearch"];

fn valid_peer(peer: &str) -> bool {
    if peer == "*" {
        return true;
    }
    let digits = peer.strip_prefix('-').unwrap_or(peer);
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && digits.bytes().any(|b| b != b'0')
}

/// Expand a leading `~` to $HOME, as a shell would.
pub fn expand_tilde(p: &str) -> PathBuf {
    let home = std::env::var_os("HOME");
    match (p.strip_prefix('~'), home) {
        (Some(""), Some(h)) => PathBuf::from(h),
        (Some(rest), Some(h)) if rest.starts_with('/') => {
            PathBuf::from(h).join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(p),
    }
}

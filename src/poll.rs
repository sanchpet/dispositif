//! One poll cycle: list dialogs, read what is new, let the allowlist decide.

use std::collections::BTreeSet;
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;
use serde_json::json;

use crate::allowlist::match_rule;
use crate::config::Config;
use crate::mcp::Mcp;
use crate::state::{AGENT_IDS_KEPT, State, StateDir};
use crate::tg::{DialogList, Message, MessageList};

/// An admitted message, as `watch` prints it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    pub event: &'static str,
    pub trust: String,
    pub rule: String,
    pub peer: String,
    pub chat: String,
    pub id: i64,
    pub from: Option<String>,
    #[serde(rename = "fromId")]
    pub from_id: Option<i64>,
    #[serde(rename = "replyTo")]
    pub reply_to: Option<i64>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub text: String,
}

/// Read every dialog that may hold something new and hand each admitted message
/// to `on_event`, marking it read. `started` is the process start (unix seconds):
/// on first sight of a chat, older history is not replayed.
pub fn poll(
    mcp: &mut Mcp,
    cfg: &Config,
    state: &mut State,
    started: f64,
    on_event: &mut dyn FnMut(Event),
) -> Result<()> {
    let explicit: BTreeSet<&str> = cfg
        .rules
        .iter()
        .filter(|r| r.peer != "*")
        .map(|r| r.peer.as_str())
        .collect();
    let dialogs: DialogList = mcp.call_as("tg_dialogs_list", json!({"limit": 100}))?;
    for d in &dialogs.dialogs {
        let peer = d.peer.as_str();
        // No skipping by dialog type: a discussion group is a supergroup, which
        // MTProto reports like a channel. Broadcast posts simply match no rule.
        if d.unread_count.unwrap_or(0) == 0
            && !explicit.contains(peer)
            && state.last.contains_key(peer)
        {
            continue;
        }
        let mut msgs = mcp
            .call_as::<MessageList>(
                "tg_messages_list",
                json!({"peer": peer, "limit": 30, "format": "json"}),
            )?
            .messages;
        msgs.sort_by_key(|m| m.id);
        let mut agent_ids: BTreeSet<i64> = state
            .agent_ids
            .get(peer)
            .into_iter()
            .flatten()
            .copied()
            .collect();
        agent_ids.extend(
            msgs.iter()
                .filter(|m| m.from_id == Some(cfg.agent_id))
                .map(|m| m.id),
        );
        let last = match state.last.get(peer) {
            Some(&last) => last,
            None => msgs
                .iter()
                .filter(|m| m.date.unwrap_or(0.0) < started)
                .map(|m| m.id)
                .max()
                .unwrap_or(0),
        };
        state.last.insert(peer.to_owned(), last);
        let is_dm = d.kind.as_deref() == Some("user");
        for m in msgs.iter().filter(|m| m.id > last) {
            learn_reply_target(mcp, cfg, peer, m, &mut agent_ids)?;
            // Consumed before the mark: a failed mark must not bring the event back.
            state.last.insert(peer.to_owned(), m.id);
            let Some(rule) = match_rule(cfg, peer, m, &agent_ids, is_dm) else {
                continue;
            };
            on_event(Event {
                event: "message",
                trust: rule.trust.clone(),
                rule: rule.name.clone(),
                peer: peer.to_owned(),
                chat: d.title.as_deref().unwrap_or("").trim().to_owned(),
                id: m.id,
                from: m.from_name.clone(),
                from_id: m.from_id,
                reply_to: m.reply_to_id(),
                kind: m.kind.clone(),
                text: m.text().to_owned(),
            });
            if let Err(e) = mcp.call(
                "tg_messages_mark_read",
                json!({"peer": peer, "maxId": m.id}),
            ) {
                crate::log(&format!("mark read failed peer={peer} msg={}: {e:#}", m.id));
            }
        }
        let kept: Vec<i64> = agent_ids.into_iter().collect();
        let skip = kept.len().saturating_sub(AGENT_IDS_KEPT);
        state
            .agent_ids
            .insert(peer.to_owned(), kept[skip..].to_vec());
    }
    Ok(())
}

/// A reply may point at an agent message older than the fetched window: look it up.
fn learn_reply_target(
    mcp: &mut Mcp,
    cfg: &Config,
    peer: &str,
    msg: &Message,
    known: &mut BTreeSet<i64>,
) -> Result<()> {
    let Some(target) = msg.reply_to_id() else {
        return Ok(());
    };
    if known.contains(&target) {
        return Ok(());
    }
    let got: MessageList = mcp.call_as(
        "tg_messages_get",
        json!({"peer": peer, "ids": [target], "format": "json"}),
    )?;
    known.extend(
        got.messages
            .iter()
            .filter(|m| m.from_id == Some(cfg.agent_id))
            .map(|m| m.id),
    );
    Ok(())
}

/// One poll in its own short MCP session, then persist state. The session is
/// closed whether or not the poll succeeded.
pub fn poll_cycle(
    mcp: &mut Mcp,
    cfg: &Config,
    dir: &StateDir,
    state: &mut State,
    started: f64,
    on_event: &mut dyn FnMut(Event),
) -> Result<()> {
    let res = mcp
        .connect()
        .and_then(|()| poll(mcp, cfg, state, started, on_event))
        .and_then(|()| dir.save_state(state));
    mcp.close();
    res
}

/// Poll health across cycles: a failure is reported once, and so is the recovery.
#[derive(Debug, Default)]
pub struct Health {
    failing: bool,
}

#[derive(Debug, PartialEq)]
pub enum Change {
    Failed(String),
    Recovered,
}

impl Health {
    /// What to report after a cycle that ended with `res`, if anything.
    pub fn observe(&mut self, res: &Result<()>) -> Option<Change> {
        match (res, self.failing) {
            (Ok(()), true) => {
                self.failing = false;
                Some(Change::Recovered)
            }
            (Err(e), false) => {
                self.failing = true;
                Some(Change::Failed(format!("{e:#}")))
            }
            _ => None,
        }
    }
}

/// `watch`'s error and recovery lines, keyed like the events: `event` first.
#[derive(Serialize)]
struct Status<'a> {
    event: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

impl Change {
    /// The JSON line `watch` prints for this change.
    pub fn to_json(&self) -> String {
        let status = match self {
            Change::Failed(e) => Status {
                event: "error",
                error: Some(crate::truncate_chars(e, 300)),
            },
            Change::Recovered => Status {
                event: "recovered",
                error: None,
            },
        };
        serde_json::to_string(&status).expect("a struct of strings always encodes")
    }
}

fn emit(v: &impl Serialize) {
    match serde_json::to_string(v) {
        Ok(line) => println!("{line}"),
        Err(e) => crate::log(&format!("cannot encode event: {e}")),
    }
}

/// `dispositif watch`: print one JSON line per admitted event; errors and
/// recovery are reported once each. With `once`, a failed cycle is an error.
pub fn watch(cfg: &Config, dir: &StateDir, once: bool) -> Result<()> {
    let started = crate::now();
    let mut state = dir.load_state()?;
    let mut mcp = Mcp::new(&cfg.mcp_url);
    let mut health = Health::default();
    loop {
        let res = poll_cycle(&mut mcp, cfg, dir, &mut state, started, &mut |ev| emit(&ev));
        if let Some(change) = health.observe(&res) {
            println!("{}", change.to_json());
        }
        if once {
            return res;
        }
        std::thread::sleep(Duration::from_secs(cfg.interval_secs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn failure_and_recovery_reported_once_each() {
        let mut h = Health::default();
        let fail = || Err(anyhow!("initialize").context("poll"));
        let seen: Vec<Option<Change>> = [fail(), fail(), Ok(()), Ok(()), fail()]
            .iter()
            .map(|r| h.observe(r))
            .collect();
        assert_eq!(
            seen,
            [
                Some(Change::Failed("poll: initialize".into())),
                None,
                Some(Change::Recovered),
                None,
                Some(Change::Failed("poll: initialize".into())),
            ]
        );
    }

    #[test]
    fn status_lines() {
        assert_eq!(
            Change::Failed("boom".into()).to_json(),
            r#"{"event":"error","error":"boom"}"#
        );
        assert_eq!(Change::Recovered.to_json(), r#"{"event":"recovered"}"#);
        let long = Change::Failed("ж".repeat(400)).to_json();
        assert_eq!(long.chars().filter(|&c| c == 'ж').count(), 300);
    }
}

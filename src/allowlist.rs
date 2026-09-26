//! What gets through. Pure functions: the allowlist is decided here, before any
//! model sees the message, so a sender who is not on it cannot steer the agent.

use std::collections::BTreeSet;

use crate::config::{Config, Rule, Trigger};
use crate::tg::Message;

/// The message mentions the agent by username, or replies to one of its messages.
pub fn is_addressed(msg: &Message, cfg: &Config, agent_msg_ids: &BTreeSet<i64>) -> bool {
    let mention = format!("@{}", cfg.agent_username.to_lowercase());
    if msg.text().to_lowercase().contains(&mention) {
        return true;
    }
    msg.reply_to_id()
        .is_some_and(|id| agent_msg_ids.contains(&id))
}

/// First rule that admits this message, or None. The agent's own messages never match;
/// in a direct message `mention_or_reply` is satisfied by the conversation itself.
pub fn match_rule<'a>(
    cfg: &'a Config,
    peer: &str,
    msg: &Message,
    agent_msg_ids: &BTreeSet<i64>,
    is_dm: bool,
) -> Option<&'a Rule> {
    let sender = msg.from_id?;
    if sender == cfg.agent_id {
        return None;
    }
    cfg.rules.iter().find(|rule| {
        (rule.peer == "*" || rule.peer == peer)
            && rule.from.contains(&sender)
            && (rule.trigger == Trigger::Any || is_dm || is_addressed(msg, cfg, agent_msg_ids))
    })
}

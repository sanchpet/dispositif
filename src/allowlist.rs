//! What gets through. Pure functions: the allowlist is decided here, before any
//! model sees the message, so a sender who is not on it cannot steer the agent.

use std::collections::BTreeSet;

use crate::config::{Config, Rule, Sender, Trigger};
use crate::tg::Message;

/// The message mentions the agent by username, or replies to one of its messages.
pub fn is_addressed(msg: &Message, cfg: &Config, agent_msg_ids: &BTreeSet<i64>) -> bool {
    let mention = format!("@{}", cfg.agent_username.to_lowercase());
    let text = msg.text().to_lowercase();
    // "@agent_bot" names another account: the mention must end the username.
    let mentioned = text.match_indices(&mention).any(|(at, _)| {
        !text[at + mention.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    });
    if mentioned {
        return true;
    }
    msg.reply_to_id()
        .is_some_and(|id| agent_msg_ids.contains(&id))
}

/// First rule that admits this message, or None. The agent's own messages never match;
/// in a direct message `mention_or_reply` is satisfied by the conversation itself.
/// A rule for users never admits a channel with the same numeric id, nor the reverse.
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
    let kind = match msg.from_type.as_deref() {
        None | Some("user") => Sender::User,
        Some("channel") => Sender::Channel,
        Some(_) => return None,
    };
    let chars = msg.text().chars().count();
    cfg.rules.iter().find(|rule| {
        (rule.peer == "*" || rule.peer == peer)
            && rule.sender == kind
            && rule.from.contains(&sender)
            && chars >= rule.min_chars
            && (rule.trigger == Trigger::Any || is_dm || is_addressed(msg, cfg, agent_msg_ids))
    })
}

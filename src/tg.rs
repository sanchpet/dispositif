//! The slice of mcp-tg's JSON output this crate reads. Unknown fields are ignored.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dialog {
    pub peer: String,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub unread_count: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: i64,
    #[serde(default)]
    pub from_id: Option<i64>,
    #[serde(default)]
    pub from_name: Option<String>,
    /// "user", "channel" or "chat". A channel's id shares the numeric space of
    /// user ids, so the id alone does not say who sent a message.
    #[serde(default)]
    pub from_type: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    /// Unix seconds.
    #[serde(default)]
    pub date: Option<f64>,
    #[serde(default)]
    pub reply_to: Option<ReplyTo>,
    #[serde(default)]
    pub forward: Option<Forward>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyTo {
    #[serde(default)]
    pub message_id: Option<i64>,
}

/// A forwarded message's origin. A channel post auto-forwarded into its discussion
/// group carries the post number and the channel's username.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Forward {
    #[serde(default)]
    pub channel_post: Option<i64>,
    #[serde(default)]
    pub from: Option<ForwardFrom>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ForwardFrom {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub peer: Option<ForwardPeer>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ForwardPeer {
    #[serde(default)]
    pub id: Option<i64>,
}

impl Message {
    pub fn reply_to_id(&self) -> Option<i64> {
        self.reply_to.as_ref().and_then(|r| r.message_id)
    }

    pub fn text(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }

    /// Sent on behalf of a channel, as its posts appear in a discussion group.
    pub fn from_channel(&self) -> bool {
        self.from_type.as_deref() == Some("channel")
    }

    /// Link to the channel post this message forwards, if it forwards one. A
    /// private channel has no username and is linked by its bare id.
    pub fn post_link(&self) -> Option<String> {
        let f = self.forward.as_ref()?;
        let (post, from) = (f.channel_post?, f.from.as_ref()?);
        match (&from.username, from.peer.as_ref().and_then(|p| p.id)) {
            (Some(user), _) => Some(format!("https://t.me/{user}/{post}")),
            (None, Some(id)) => Some(format!("https://t.me/c/{id}/{post}")),
            (None, None) => None,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct DialogList {
    #[serde(default)]
    pub dialogs: Vec<Dialog>,
}

#[derive(Debug, Default, Deserialize)]
pub struct MessageList {
    #[serde(default)]
    pub messages: Vec<Message>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_links() {
        let m = |fwd: serde_json::Value| -> Message {
            serde_json::from_value(serde_json::json!({"id": 1, "forward": fwd})).unwrap()
        };
        let public = m(
            serde_json::json!({"channelPost": 319, "from": {"username": "diary", "peer": {"id": 42}}}),
        );
        assert_eq!(
            public.post_link().as_deref(),
            Some("https://t.me/diary/319")
        );
        let private = m(serde_json::json!({"channelPost": 5, "from": {"peer": {"id": 42}}}));
        assert_eq!(private.post_link().as_deref(), Some("https://t.me/c/42/5"));
        assert_eq!(
            m(serde_json::json!({"from": {"username": "diary"}})).post_link(),
            None
        );
        let plain: Message = serde_json::from_value(serde_json::json!({"id": 1})).unwrap();
        assert_eq!(plain.post_link(), None);
    }
}

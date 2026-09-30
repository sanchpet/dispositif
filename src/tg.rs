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

    /// Public link to the channel post this message forwards, if it forwards one.
    pub fn post_link(&self) -> Option<String> {
        let f = self.forward.as_ref()?;
        let user = f.from.as_ref()?.username.as_deref()?;
        Some(format!("https://t.me/{user}/{}", f.channel_post?))
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

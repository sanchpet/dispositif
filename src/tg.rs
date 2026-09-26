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
    #[serde(default)]
    pub text: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    /// Unix seconds.
    #[serde(default)]
    pub date: Option<f64>,
    #[serde(default)]
    pub reply_to: Option<ReplyTo>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyTo {
    #[serde(default)]
    pub message_id: Option<i64>,
}

impl Message {
    pub fn reply_to_id(&self) -> Option<i64> {
        self.reply_to.as_ref().and_then(|r| r.message_id)
    }

    pub fn text(&self) -> &str {
        self.text.as_deref().unwrap_or("")
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

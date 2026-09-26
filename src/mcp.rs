//! A minimal MCP streamable-HTTP client for the mcp-tg daemon.
//!
//! Sessions are meant to be short: `connect`, a few `call`s, `close`. The daemon
//! keepalive-pings idle sessions over an SSE stream this client never opens and
//! closes a session once a ping cannot be delivered; a session that lives for one
//! poll cycle is never pinged.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

pub const PROTOCOL_VERSION: &str = "2025-11-25";
const SESSION_HEADER: &str = "Mcp-Session-Id";

pub struct Mcp {
    url: String,
    agent: ureq::Agent,
    sid: Option<String>,
    next_id: u64,
}

impl Mcp {
    pub fn new(url: &str) -> Mcp {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(60)))
            .build()
            .into();
        Mcp {
            url: url.to_owned(),
            agent,
            sid: None,
            next_id: 0,
        }
    }

    fn post(&mut self, body: &Value) -> Result<Option<Value>> {
        let mut req = self
            .agent
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");
        if let Some(sid) = &self.sid {
            req = req.header(SESSION_HEADER, sid);
        }
        let mut resp = req.send(body.to_string())?;
        if let Some(sid) = resp.headers().get(SESSION_HEADER) {
            self.sid = Some(sid.to_str()?.to_owned());
        }
        let raw = resp.body_mut().read_to_string()?;
        parse_body(&raw)
    }

    /// Open a fresh session: initialize, then notifications/initialized.
    pub fn connect(&mut self) -> Result<()> {
        self.sid = None;
        self.post(&json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION")},
            },
        }))
        .context("initialize")?;
        self.post(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .context("notifications/initialized")?;
        Ok(())
    }

    /// End the session with an HTTP DELETE. Best effort: a failure here only means
    /// the daemon reaps the session itself later.
    pub fn close(&mut self) {
        let Some(sid) = self.sid.take() else { return };
        let _ = self
            .agent
            .delete(&self.url)
            .header(SESSION_HEADER, &sid)
            .config()
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .call();
    }

    /// Call a tool and return its structured result.
    pub fn call(&mut self, tool: &str, args: Value) -> Result<Value> {
        self.next_id += 1;
        let resp = self
            .post(&json!({
                "jsonrpc": "2.0", "id": self.next_id, "method": "tools/call",
                "params": {"name": tool, "arguments": args},
            }))
            .with_context(|| tool.to_owned())?
            .ok_or_else(|| anyhow!("{tool}: empty response"))?;
        if let Some(err) = resp.get("error") {
            bail!("{tool}: {err}");
        }
        tool_result(tool, resp)
    }

    /// `call`, deserialized into `T`.
    pub fn call_as<T: DeserializeOwned>(&mut self, tool: &str, args: Value) -> Result<T> {
        let v = self.call(tool, args)?;
        serde_json::from_value(v).with_context(|| format!("{tool}: unexpected result shape"))
    }
}

fn tool_result(tool: &str, resp: Value) -> Result<Value> {
    let res = resp
        .get("result")
        .ok_or_else(|| anyhow!("{tool}: response has neither result nor error"))?;
    let first_text = || {
        res.pointer("/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or("")
    };
    if res.get("isError").and_then(Value::as_bool) == Some(true) {
        bail!("{tool}: {}", first_text());
    }
    match res.get("structuredContent") {
        Some(sc) if !is_empty(sc) => Ok(sc.clone()),
        _ => serde_json::from_str(first_text())
            .with_context(|| format!("{tool}: result is not JSON")),
    }
}

fn is_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Object(m) => m.is_empty(),
        _ => false,
    }
}

/// Parse a response body that is either plain JSON or an SSE stream. An SSE
/// event's `data:` lines are joined with newlines and events with no data (a
/// reconnection primer) are skipped. The first JSON-RPC response (a message
/// with `result` or `error`) wins, so a notification sent ahead of it is skipped.
pub fn parse_body(raw: &str) -> Result<Option<Value>> {
    if !raw.lines().any(|l| l.starts_with("data:")) {
        if raw.trim().is_empty() {
            return Ok(None);
        }
        return Ok(Some(
            serde_json::from_str(raw).context("response is not JSON")?,
        ));
    }
    let mut events = Vec::new();
    let mut data: Vec<&str> = Vec::new();
    for line in raw.lines().chain([""]) {
        if line.is_empty() {
            let event = data.join("\n");
            data.clear();
            if !event.trim().is_empty() {
                events.push(event);
            }
        } else if let Some(d) = line.strip_prefix("data:") {
            data.push(d.strip_prefix(' ').unwrap_or(d));
        }
    }
    let mut parsed = Vec::with_capacity(events.len());
    for e in &events {
        parsed.push(serde_json::from_str::<Value>(e).context("SSE data is not JSON")?);
    }
    if parsed.is_empty() {
        return Ok(None);
    }
    let pick = parsed
        .iter()
        .position(|v| v.get("result").is_some() || v.get("error").is_some())
        .unwrap_or(0);
    Ok(Some(parsed.swap_remove(pick)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_json() {
        let v = parse_body(r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#)
            .unwrap()
            .unwrap();
        assert_eq!(v["result"]["ok"], true);
    }

    #[test]
    fn empty_body_is_none() {
        assert!(parse_body("").unwrap().is_none());
        assert!(parse_body("  \n").unwrap().is_none());
    }

    #[test]
    fn sse_single_event() {
        let raw = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"n\":7}}\n\n";
        assert_eq!(parse_body(raw).unwrap().unwrap()["result"]["n"], 7);
    }

    #[test]
    fn sse_skips_notification_before_response() {
        let raw = concat!(
            "event: message\n",
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n",
            "event: message\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":4,\"result\":{\"n\":1}}\n\n",
        );
        assert_eq!(parse_body(raw).unwrap().unwrap()["id"], 4);
    }

    #[test]
    fn sse_skips_empty_primer_event() {
        let raw = "id: 1\ndata:\n\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":5,\"result\":{}}\n\n";
        assert_eq!(parse_body(raw).unwrap().unwrap()["id"], 5);
    }

    #[test]
    fn sse_joins_multiline_data() {
        let raw = "event: message\ndata: {\"jsonrpc\":\"2.0\",\ndata: \"id\":6,\"result\":{}}\n\n";
        assert_eq!(parse_body(raw).unwrap().unwrap()["id"], 6);
    }

    #[test]
    fn sse_with_only_empty_data_is_none() {
        assert!(parse_body("id: 1\ndata:\n\n").unwrap().is_none());
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(parse_body("<html>").is_err());
        assert!(parse_body("data: nope").is_err());
    }

    #[test]
    fn structured_content_preferred_text_fallback() {
        let r =
            json!({"result": {"structuredContent": {"a": 1}, "content": [{"text": "{\"a\":2}"}]}});
        assert_eq!(tool_result("t", r).unwrap()["a"], 1);
        let r = json!({"result": {"structuredContent": {}, "content": [{"text": "{\"a\":2}"}]}});
        assert_eq!(tool_result("t", r).unwrap()["a"], 2);
    }

    #[test]
    fn tool_error_surfaces_text() {
        let r = json!({"result": {"isError": true, "content": [{"text": "peer not found"}]}});
        let e = tool_result("tg_x", r).unwrap_err().to_string();
        assert_eq!(e, "tg_x: peer not found");
    }
}

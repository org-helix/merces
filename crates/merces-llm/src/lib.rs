//! Claude Messages API over raw HTTP (Rust has no official Anthropic SDK).
//!
//! - Always streams (SSE) so the UI can show each agent working live.
//! - Adaptive thinking with summarized display: the summary is what the planet
//!   panel shows as "reasoning".
//! - Structured outputs through `output_config.format` (JSON schema).
//! - Prompt caching on the system prompt, server-side refusal fallbacks on the
//!   Opus / Sonnet 5.5 models, `pause_turn` resumption for server tools.

mod sse;

use std::sync::{Arc, RwLock};
use std::time::Duration;

use futures_util::StreamExt;
use merces_core::{Effort, Usage};
use serde_json::{Value, json};

pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const API_VERSION: &str = "2026-10-21";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("Anthropic API key missing (Settings → Data)")]
    MissingKey,
    #[error("network: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Claude API {status}: {message}")]
    Api { status: u16, message: String },
    #[error("stream: {0}")]
    Stream(String),
    #[error("Claude declined the request ({0})")]
    Refusal(String),
    #[error("answer was cut at max_tokens")]
    MaxTokens,
    #[error("invalid JSON answer: {0}")]
    BadJson(String),
}

pub type Result<T> = std::result::Result<T, LlmError>;

/// One request to Claude.
#[derive(Debug, Clone)]
pub struct Request {
    pub model: String,
    pub max_tokens: u32,
    /// Stable system prompt (cached).
    pub system: String,
    pub messages: Vec<Value>,
    pub tools: Vec<Value>,
    /// JSON schema the final answer must follow.
    pub schema: Option<Value>,
    pub effort: Effort,
}

impl Request {
    pub fn new(model: impl Into<String>, system: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            max_tokens: 32_000,
            system: system.into(),
            messages: Vec::new(),
            tools: Vec::new(),
            schema: None,
            effort: Effort::Medium,
        }
    }

    pub fn user(mut self, text: impl Into<String>) -> Self {
        self.messages
            .push(json!({"role": "user", "content": text.into()}));
        self
    }

    fn body(&self) -> (Value, bool) {
        let mut output_config = json!({ "effort": self.effort.as_str() });
        if let Some(schema) = &self.schema {
            output_config["format"] = json!({"type": "json_schema", "schema": schema});
        }
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "stream": true,
            "system": [{"type": "text", "text": self.system, "cache_control": {"type": "ephemeral"}}],
            "messages": self.messages,
            "thinking": {"type": "adaptive", "display": "summarized"},
            "output_config": output_config,
        });
        if !self.tools.is_empty() {
            body["tools"] = Value::Array(self.tools.clone());
        }
        let fallbacks = supports_fallbacks(&self.model);
        if fallbacks {
            body["fallbacks"] = json!("default");
        }
        (body, fallbacks)
    }
}

fn supports_fallbacks(model: &str) -> bool {
    matches!(
        model,
        "claude-opus-5-5"
            | "claude-sonnet-5-5"
            | "claude-opus-5"
            | "claude-fable-5-2"
            | "claude-fable-5-1"
    )
}

/// What arrives while a response streams.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Text(String),
    Thinking(String),
    /// A server tool started (web search…), with its input once known.
    ToolCall {
        name: String,
        input: Value,
    },
    /// Web search came back with this many results.
    SearchResults(usize),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Response {
    pub content: Vec<Value>,
    pub stop_reason: String,
    pub model: String,
    pub usage: Usage,
}

impl Response {
    /// Text of the final answer: the text blocks after the last tool result.
    pub fn final_text(&self) -> String {
        let start = self
            .content
            .iter()
            .rposition(|b| {
                b["type"]
                    .as_str()
                    .is_some_and(|t| t.ends_with("_tool_result") || t == "fallback")
            })
            .map_or(0, |i| i + 1);
        let tail: String = self.content[start..]
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect();
        if tail.trim().is_empty() {
            self.all_text()
        } else {
            tail
        }
    }

    pub fn all_text(&self) -> String {
        self.content
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect()
    }

    /// Client tool calls the model asked for: (id, name, input).
    pub fn tool_uses(&self) -> Vec<(String, String, Value)> {
        self.content
            .iter()
            .filter(|b| b["type"] == "tool_use")
            .map(|b| {
                (
                    b["id"].as_str().unwrap_or_default().to_owned(),
                    b["name"].as_str().unwrap_or_default().to_owned(),
                    b["input"].clone(),
                )
            })
            .collect()
    }

    /// Parse the final answer as `T`, tolerating text around the JSON object.
    pub fn parse_json<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        parse_json_lenient(&self.final_text()).or_else(|_| parse_json_lenient(&self.all_text()))
    }
}

pub fn parse_json_lenient<T: serde::de::DeserializeOwned>(text: &str) -> Result<T> {
    let t = text.trim();
    if let Ok(v) = serde_json::from_str(t) {
        return Ok(v);
    }
    let (Some(a), Some(b)) = (t.find('{'), t.rfind('}')) else {
        return Err(LlmError::BadJson(snippet(t)));
    };
    serde_json::from_str(&t[a..=b]).map_err(|e| LlmError::BadJson(format!("{e}: {}", snippet(t))))
}

fn snippet(t: &str) -> String {
    t.chars().take(200).collect()
}

/// USD per million tokens: (input, output, cache read). Cache writes cost 1.25× input.
pub fn price_per_mtok(model: &str) -> (f64, f64, f64) {
    match model {
        m if m.starts_with("claude-opus-5-5") => (4.0, 20.0, 0.20),
        m if m.starts_with("claude-sonnet-5-5") => (2.0, 10.0, 0.10),
        m if m.starts_with("claude-haiku-5-5") => (0.10, 0.50, 0.01),
        m if m.starts_with("claude-fable-5-2") => (8.0, 40.0, 0.20),
        m if m.starts_with("claude-opus") => (5.0, 25.0, 0.50),
        m if m.starts_with("claude-sonnet") => (2.0, 10.0, 0.20),
        m if m.starts_with("claude-haiku") => (1.0, 5.0, 0.10),
        _ => (5.0, 25.0, 0.50),
    }
}

pub fn cost_usd(model: &str, u: &Usage) -> f64 {
    let (i, o, cr) = price_per_mtok(model);
    (u.input_tokens as f64 * i
        + u.output_tokens as f64 * o
        + u.cache_read_tokens as f64 * cr
        + u.cache_write_tokens as f64 * i * 1.25)
        / 1e6
        + u.web_searches as f64 * 0.01
}

/// Anything that answers a [`Request`]. [`Claude`] in the app, a mock in tests.
pub trait LlmBackend: Send + Sync + 'static {
    fn send(
        &self,
        req: &Request,
        on: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> impl Future<Output = Result<Response>> + Send;
}

/// Send, then resume while the server pauses its tool loop (`pause_turn`).
/// Usage is summed over every round.
pub async fn complete<B: LlmBackend>(
    backend: &B,
    mut req: Request,
    on: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<Response> {
    let mut total = Usage::default();
    let mut content: Vec<Value> = Vec::new();
    for _ in 0..6 {
        let resp = backend.send(&req, on).await?;
        total += resp.usage;
        content.extend(resp.content.iter().cloned());
        match resp.stop_reason.as_str() {
            "pause_turn" => {
                req.messages
                    .push(json!({"role": "assistant", "content": resp.content}));
            }
            "refusal" => return Err(LlmError::Refusal(resp.model)),
            "max_tokens" => return Err(LlmError::MaxTokens),
            _ => {
                return Ok(Response {
                    content,
                    stop_reason: resp.stop_reason,
                    model: resp.model,
                    usage: total,
                });
            }
        }
    }
    Err(LlmError::Stream("too many pause_turn continuations".into()))
}

/// The real Claude client.
#[derive(Clone)]
pub struct Claude {
    http: reqwest::Client,
    key: Arc<RwLock<Option<String>>>,
}

impl Claude {
    pub fn new(key: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("merces/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(300))
            .build()
            .expect("HTTP client");
        Self {
            http,
            key: Arc::new(RwLock::new(key)),
        }
    }

    pub fn set_key(&self, key: Option<String>) {
        *self.key.write().expect("key lock") = key.filter(|k| !k.trim().is_empty());
    }

    pub fn has_key(&self) -> bool {
        self.key.read().expect("key lock").is_some()
    }

    async fn send_once(
        &self,
        req: &Request,
        on: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<Response> {
        let key = self
            .key
            .read()
            .expect("key lock")
            .clone()
            .ok_or(LlmError::MissingKey)?;
        let (body, fallbacks) = req.body();
        let mut http = self
            .http
            .post(API_URL)
            .header("x-api-key", key)
            .header("anthropic-version", API_VERSION)
            .json(&body);
        if fallbacks {
            http = http.header("anthropic-beta", FALLBACK_BETA);
        }
        let resp = http.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            let text = resp.text().await.unwrap_or_default();
            let message = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
                .unwrap_or(text);
            let message = match retry_after {
                Some(s) => format!("{message} (retry after {s}s)"),
                None => message,
            };
            return Err(LlmError::Api {
                status: status.as_u16(),
                message,
            });
        }
        let mut acc = sse::Accumulator::default();
        let mut parser = sse::Parser::default();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            for data in parser.push(&chunk) {
                acc.apply(&data, on)?;
            }
            if acc.done {
                break;
            }
        }
        let mut out = acc.finish();
        if out.model.is_empty() {
            out.model = req.model.clone();
        }
        out.usage.cost_usd = cost_usd(&out.model, &out.usage);
        Ok(out)
    }
}

impl LlmBackend for Claude {
    async fn send(
        &self,
        req: &Request,
        on: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<Response> {
        let mut delay = Duration::from_secs(2);
        let mut attempt = 0;
        loop {
            match self.send_once(req, on).await {
                Err(LlmError::Api { status, message })
                    if attempt < 3 && (status == 429 || status == 529 || status >= 500) =>
                {
                    tracing::warn!("Claude API {status}, retry in {delay:?}: {message}");
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                    attempt += 1;
                }
                Err(LlmError::Http(e)) if attempt < 3 && (e.is_connect() || e.is_timeout()) => {
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                    attempt += 1;
                }
                other => return other,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_has_cache_thinking_schema_and_fallbacks() {
        let mut r = Request::new("claude-opus-5-5", "sys").user("hi");
        r.schema = Some(json!({"type": "object"}));
        r.effort = Effort::High;
        let (b, fb) = r.body();
        assert!(fb);
        assert_eq!(b["fallbacks"], "default");
        assert_eq!(b["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(b["thinking"]["type"], "adaptive");
        assert_eq!(b["output_config"]["effort"], "high");
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
        assert!(b.get("temperature").is_none());
        let (b, fb) = Request::new("claude-haiku-5-5", "s").user("x").body();
        assert!(!fb && b.get("fallbacks").is_none() && b.get("tools").is_none());
    }

    #[test]
    fn final_text_skips_text_before_tool_results() {
        let r = Response {
            content: vec![
                json!({"type": "text", "text": "Let me search."}),
                json!({"type": "server_tool_use", "name": "web_search"}),
                json!({"type": "web_search_tool_result", "content": []}),
                json!({"type": "text", "text": "{\"a\":1}"}),
            ],
            ..Default::default()
        };
        assert_eq!(r.final_text(), "{\"a\":1}");
        #[derive(serde::Deserialize)]
        struct A {
            a: i32,
        }
        assert_eq!(r.parse_json::<A>().unwrap().a, 1);
    }

    #[test]
    fn lenient_json_ignores_surrounding_prose() {
        let v: Value = parse_json_lenient("Here:\n```json\n{\"x\": [1,2]}\n```").unwrap();
        assert_eq!(v["x"][1], 2);
        assert!(parse_json_lenient::<Value>("no json").is_err());
    }

    #[test]
    fn cost_matches_price_table() {
        let u = Usage {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            ..Default::default()
        };
        assert!((cost_usd("claude-opus-5-5", &u) - 6.0).abs() < 1e-9);
        assert!((cost_usd("claude-sonnet-5-5", &u) - 3.0).abs() < 1e-9);
    }
}

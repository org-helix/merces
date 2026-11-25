//! Server-sent events parsing and response reconstruction.

use serde_json::{Map, Value, json};

use crate::{LlmError, Response, Result, StreamEvent};

/// Splits a byte stream into the `data:` payloads of complete SSE events.
#[derive(Default)]
pub struct Parser {
    buf: String,
}

impl Parser {
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.push_str(&String::from_utf8_lossy(chunk));
        if self.buf.contains('\r') {
            self.buf = self.buf.replace("\r\n", "\n");
        }
        let mut out = Vec::new();
        while let Some(end) = self.buf.find("\n\n") {
            let event: String = self.buf.drain(..end + 2).collect();
            let data: Vec<&str> = event
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .map(str::trim_start)
                .collect();
            if !data.is_empty() {
                out.push(data.join("\n"));
            }
        }
        out
    }
}

/// Rebuilds the full message (content blocks, stop reason, usage) from events,
/// so a paused turn can be sent back verbatim.
#[derive(Default)]
pub struct Accumulator {
    blocks: Vec<Value>,
    partial_json: Vec<String>,
    stop_reason: String,
    model: String,
    usage: merces_core::Usage,
    pub done: bool,
}

impl Accumulator {
    pub fn apply(&mut self, data: &str, on: &mut (dyn FnMut(StreamEvent) + Send)) -> Result<()> {
        let ev: Value =
            serde_json::from_str(data).map_err(|e| LlmError::Stream(format!("{e}: {data}")))?;
        match ev["type"].as_str().unwrap_or_default() {
            "message_start" => {
                let m = &ev["message"];
                self.model = m["model"].as_str().unwrap_or_default().to_owned();
                self.read_usage(&m["usage"]);
            }
            "content_block_start" => {
                let i = ev["index"].as_u64().unwrap_or(0) as usize;
                while self.blocks.len() <= i {
                    self.blocks.push(Value::Null);
                    self.partial_json.push(String::new());
                }
                let block = ev["content_block"].clone();
                if block["type"] == "web_search_tool_result"
                    && let Some(results) = block["content"].as_array()
                {
                    on(StreamEvent::SearchResults(results.len()));
                }
                self.blocks[i] = block;
            }
            "content_block_delta" => {
                let i = ev["index"].as_u64().unwrap_or(0) as usize;
                let d = &ev["delta"];
                let Some(block) = self.blocks.get_mut(i) else {
                    return Ok(());
                };
                match d["type"].as_str().unwrap_or_default() {
                    "text_delta" => {
                        let t = d["text"].as_str().unwrap_or_default();
                        append(block, "text", t);
                        on(StreamEvent::Text(t.to_owned()));
                    }
                    "thinking_delta" => {
                        let t = d["thinking"].as_str().unwrap_or_default();
                        append(block, "thinking", t);
                        on(StreamEvent::Thinking(t.to_owned()));
                    }
                    "signature_delta" => {
                        append(
                            block,
                            "signature",
                            d["signature"].as_str().unwrap_or_default(),
                        );
                    }
                    "input_json_delta" => {
                        self.partial_json[i]
                            .push_str(d["partial_json"].as_str().unwrap_or_default());
                    }
                    "citations_delta" => {
                        if let Some(obj) = block.as_object_mut() {
                            let list = obj.entry("citations").or_insert_with(|| json!([]));
                            if let Some(a) = list.as_array_mut() {
                                a.push(d["citation"].clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let i = ev["index"].as_u64().unwrap_or(0) as usize;
                if let (Some(block), Some(json)) =
                    (self.blocks.get_mut(i), self.partial_json.get(i))
                {
                    let ty = block["type"].as_str().unwrap_or_default().to_owned();
                    if ty == "tool_use" || ty == "server_tool_use" {
                        let input = if json.trim().is_empty() {
                            block
                                .get("input")
                                .cloned()
                                .unwrap_or_else(|| Value::Object(Map::new()))
                        } else {
                            serde_json::from_str(json).unwrap_or_else(|_| Value::Object(Map::new()))
                        };
                        block["input"] = input.clone();
                        on(StreamEvent::ToolCall {
                            name: block["name"].as_str().unwrap_or_default().to_owned(),
                            input,
                        });
                    }
                }
            }
            "message_delta" => {
                if let Some(r) = ev["delta"]["stop_reason"].as_str() {
                    self.stop_reason = r.to_owned();
                }
                self.read_usage(&ev["usage"]);
            }
            "message_stop" => self.done = true,
            "error" => {
                let msg = ev["error"]["message"]
                    .as_str()
                    .unwrap_or("stream error")
                    .to_owned();
                let status = match ev["error"]["type"].as_str() {
                    Some("overloaded_error") => 529,
                    Some("rate_limit_error") => 429,
                    _ => 500,
                };
                return Err(LlmError::Api {
                    status,
                    message: msg,
                });
            }
            _ => {}
        }
        Ok(())
    }

    /// Usage fields are cumulative: keep the largest value seen.
    fn read_usage(&mut self, u: &Value) {
        let get = |k: &str| u[k].as_u64();
        let u2 = &mut self.usage;
        if let Some(v) = get("input_tokens") {
            u2.input_tokens = u2.input_tokens.max(v);
        }
        if let Some(v) = get("output_tokens") {
            u2.output_tokens = u2.output_tokens.max(v);
        }
        if let Some(v) = get("cache_read_input_tokens") {
            u2.cache_read_tokens = u2.cache_read_tokens.max(v);
        }
        if let Some(v) = get("cache_creation_input_tokens") {
            u2.cache_write_tokens = u2.cache_write_tokens.max(v);
        }
        if let Some(v) = u["server_tool_use"]["web_search_requests"].as_u64() {
            u2.web_searches = u2.web_searches.max(v);
        }
    }

    pub fn finish(self) -> Response {
        Response {
            content: self.blocks.into_iter().filter(|b| !b.is_null()).collect(),
            stop_reason: self.stop_reason,
            model: self.model,
            usage: self.usage,
        }
    }
}

fn append(block: &mut Value, field: &str, text: &str) {
    if let Some(obj) = block.as_object_mut() {
        let entry = obj
            .entry(field)
            .or_insert_with(|| Value::String(String::new()));
        if let Value::String(s) = entry {
            s.push_str(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuilds_message_from_split_chunks() {
        let raw = concat!(
            "event: message_start\r\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-opus-5-5\",\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":5,\"output_tokens\":1}}}\r\n\r\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Hmm\"}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"server_tool_use\",\"id\":\"s1\",\"name\":\"web_search\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\":\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"btc\\\"}\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":42,\"server_tool_use\":{\"web_search_requests\":1}}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );
        let mut p = Parser::default();
        let mut acc = Accumulator::default();
        let mut events = Vec::new();
        // Feed in awkward 7-byte chunks.
        for chunk in raw.as_bytes().chunks(7) {
            for d in p.push(chunk) {
                acc.apply(&d, &mut |e| events.push(e)).unwrap();
            }
        }
        assert!(acc.done);
        let r = acc.finish();
        assert_eq!(r.stop_reason, "end_turn");
        assert_eq!(r.all_text(), "Hello");
        assert_eq!(r.content[0]["thinking"], "Hmm");
        assert_eq!(r.content[1]["input"]["query"], "btc");
        assert_eq!(r.usage.input_tokens, 10);
        assert_eq!(r.usage.cache_read_tokens, 5);
        assert_eq!(r.usage.output_tokens, 42);
        assert_eq!(r.usage.web_searches, 1);
        assert!(events.contains(&StreamEvent::Thinking("Hmm".into())));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::ToolCall { name, .. } if name == "web_search"))
        );
    }

    #[test]
    fn stream_error_event_is_an_error() {
        let mut acc = Accumulator::default();
        let r = acc.apply(
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            &mut |_| {},
        );
        assert!(matches!(r, Err(LlmError::Api { status: 529, .. })));
    }
}

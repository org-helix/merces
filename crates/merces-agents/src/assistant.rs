//! The assistant: answers questions about the app and trading, and can propose
//! symbols (confirmed by the user in the UI).

use chrono::Utc;
use merces_core::{AgentId, AgentOutput, AssetClass, EngineEvent, Provider, SymbolConfig};
use merces_llm::{LlmBackend, Request, StreamEvent};
use merces_market::MarketSource;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Ctx, prompts, schemas};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatTurn {
    pub user: bool,
    pub text: String,
}

fn context<L: LlmBackend, M: MarketSource>(ctx: &Ctx<L, M>) -> String {
    let cfg = ctx.config();
    let mut s = String::from("Current state of the app (read-only):\nSymbols:\n");
    for sym in &cfg.symbols {
        s.push_str(&format!(
            "- {} ({}, {:?}, {}), daily run {}, {}\n",
            sym.symbol,
            sym.name,
            sym.class,
            sym.provider.label(),
            sym.schedule_utc
                .map_or("manual".into(), |t| format!("{} UTC", t.format("%H:%M"))),
            if sym.enabled { "enabled" } else { "disabled" }
        ));
    }
    let midnight = Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap_or_default()
        .and_utc();
    if let Ok(outputs) = ctx.store.outputs_since(midnight) {
        if !outputs.is_empty() {
            s.push_str("\nToday's results (newest first):\n");
        }
        for o in outputs.iter().take(12) {
            let line = match &o.output {
                AgentOutput::Brief(b) => format!(
                    "research: day {:?}, week {:?}, sentiment {}",
                    b.bias_day, b.bias_week, b.sentiment
                ),
                AgentOutput::Chart(c) => format!(
                    "chart: {:?}, {:?}, confidence {}",
                    c.stance, c.bias, c.confidence
                ),
                AgentOutput::Decision(d) => format!(
                    "decision: {:?}, risk {} — {}",
                    d.action, d.risk_score, d.rationale
                ),
                AgentOutput::Plan(p) => format!(
                    "plan: {:?} entry {} SL {} TP {:?} confidence {}",
                    p.side, p.entry, p.stop_loss, p.take_profits, p.confidence
                ),
            };
            s.push_str(&format!(
                "- {} {}: {line}\n",
                o.created_at.format("%H:%M"),
                o.symbol
            ));
        }
    }
    s
}

pub async fn answer<L: LlmBackend, M: MarketSource>(ctx: &Ctx<L, M>, history: Vec<ChatTurn>) {
    let cfg = ctx.config();
    let settings = cfg.agent(AgentId::Assistant);
    let mut req = Request::new(
        settings.model.clone(),
        prompts::system(AgentId::Assistant, cfg.lang, &settings.extra_instructions),
    );
    req.effort = settings.effort;
    req.max_tokens = 8_000;
    req.tools = vec![
        schemas::propose_symbol_tool(),
        schemas::web_search(&settings.model, 3),
    ];
    let n = history.len();
    for (i, turn) in history.into_iter().enumerate() {
        // The live state rides on the last user message so the cached prefix stays stable.
        let text = if i + 1 == n && turn.user {
            format!("{}\n\n---\n{}", turn.text, context(ctx))
        } else {
            turn.text
        };
        req.messages
            .push(json!({"role": if turn.user { "user" } else { "assistant" }, "content": text}));
    }

    let sink = ctx.sink.clone();
    for _ in 0..4 {
        let mut on = |e: StreamEvent| {
            if let StreamEvent::Text(t) = e {
                sink(EngineEvent::AssistantDelta(t));
            }
        };
        let resp = match merces_llm::complete(&*ctx.llm, req.clone(), &mut on).await {
            Ok(r) => r,
            Err(e) => {
                ctx.emit(EngineEvent::Error {
                    agent: Some(AgentId::Assistant),
                    message: e.to_string(),
                });
                break;
            }
        };
        ctx.record_usage(AgentId::Assistant, resp.usage);
        let calls = resp.tool_uses();
        if resp.stop_reason != "tool_use" || calls.is_empty() {
            break;
        }
        req.messages
            .push(json!({"role": "assistant", "content": resp.content}));
        let mut results = Vec::new();
        for (id, name, input) in calls {
            let (content, is_error) = if name == "propose_symbol" {
                match parse_proposal(&input) {
                    Some(s) => {
                        ctx.emit(EngineEvent::SymbolProposal(s));
                        ("Proposal shown to the user, who will confirm or decline it in the app.".to_owned(), false)
                    }
                    None => (
                        "Invalid proposal: symbol, asset_class and provider are required."
                            .to_owned(),
                        true,
                    ),
                }
            } else {
                (format!("Unknown tool {name}"), true)
            };
            results.push(json!({"type": "tool_result", "tool_use_id": id, "content": content, "is_error": is_error}));
        }
        req.messages
            .push(json!({"role": "user", "content": results}));
        sink(EngineEvent::AssistantDelta("\n\n".into()));
    }
    ctx.emit(EngineEvent::AssistantDone);
}

fn parse_proposal(v: &Value) -> Option<SymbolConfig> {
    let symbol = v["symbol"].as_str()?.trim().to_owned();
    if symbol.is_empty() {
        return None;
    }
    let class: AssetClass = serde_json::from_value(v["asset_class"].clone()).ok()?;
    let provider: Provider = serde_json::from_value(v["provider"].clone()).ok()?;
    Some(SymbolConfig {
        name: v["name"].as_str().unwrap_or(&symbol).to_owned(),
        symbol,
        class,
        provider,
        ..SymbolConfig::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposal_parsing() {
        let s = parse_proposal(&json!({"symbol": "XAU/USD", "name": "Gold", "asset_class": "commodity", "provider": "twelve_data"}))
            .unwrap();
        assert_eq!(s.provider, Provider::TwelveData);
        assert_eq!(s.class, AssetClass::Commodity);
        assert!(
            parse_proposal(&json!({"symbol": "X", "asset_class": "nope", "provider": "binance"}))
                .is_none()
        );
    }
}

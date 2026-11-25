//! JSON schemas for Claude's structured outputs. Every object closes with
//! `additionalProperties: false` and lists all its fields as required, as the
//! structured-outputs feature expects; numeric ranges are stated in descriptions
//! (the API does not accept `minimum` / `maximum`) and checked in Rust.

use serde_json::{Value, json};

fn obj(props: Value) -> Value {
    let required: Vec<String> = props
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

fn bias() -> Value {
    json!({"type": "string", "enum": ["bullish", "bearish", "neutral"]})
}

fn text(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}

pub fn market_brief() -> Value {
    obj(json!({
        "bias_day": bias(),
        "bias_week": bias(),
        "sentiment": {"type": "integer", "description": "-100 very bearish … +100 very bullish"},
        "summary": text("Overview of the market state for this symbol, 4-8 sentences."),
        "catalysts": {"type": "array", "items": obj(json!({
            "title": text("Event or news"),
            "impact": bias(),
            "when": text("When it happens or happened")
        }))},
        "risks": {"type": "array", "items": text("A risk that could invalidate the bias")},
        "sources": {"type": "array", "items": text("URL or source name")}
    }))
}

pub fn chart_view() -> Value {
    obj(json!({
        "stance": {"type": "string", "enum": ["confirms", "contradicts", "mixed"],
                   "description": "Whether the charts confirm the research bias"},
        "bias": bias(),
        "structure": {"type": "array", "items": text("One line per timeframe, e.g. '4h: higher highs, above EMA200'")},
        "levels": {"type": "array", "items": obj(json!({
            "price": {"type": "number"},
            "kind": {"type": "string", "enum": ["support", "resistance", "pivot"]},
            "note": text("Why the level matters")
        }))},
        "atr": {"type": "number", "description": "Reference ATR(14) used for the plan, in price units"},
        "summary": text("Chart reading, 4-8 sentences."),
        "confidence": {"type": "integer", "description": "0-100"}
    }))
}

pub fn decision() -> Value {
    obj(json!({
        "action": {"type": "string", "enum": ["buy", "sell", "no_trade"]},
        "rationale": text("Why this action, citing both analyses"),
        "risk_assessment": text("Main risks today"),
        "risk_score": {"type": "integer", "description": "0 no risk … 100 extreme risk"},
        "horizon": text("Expected holding horizon, e.g. 'intraday', '2-3 days'"),
        "prediction": text("What price is expected to do and by when")
    }))
}

pub fn trade_plan() -> Value {
    obj(json!({
        "side": {"type": "string", "enum": ["long", "short"]},
        "entry": {"type": "number"},
        "entry_window_start": {"type": "string", "format": "date-time", "description": "UTC, RFC 3339"},
        "entry_window_end": {"type": "string", "format": "date-time", "description": "UTC, RFC 3339"},
        "stop_loss": {"type": "number"},
        "take_profits": {"type": "array", "items": {"type": "number"}, "description": "TP1 first, 1 to 3 targets"},
        "risk_reward": {"type": "number", "description": "Reward/risk to TP1"},
        "risk_pct": {"type": "number", "description": "Share of the account to risk, in %"},
        "confidence": {"type": "integer", "description": "0-100"},
        "notes": text("Execution details: trigger, invalidation, how to manage the trade")
    }))
}

pub fn propose_symbol_tool() -> Value {
    json!({
        "name": "propose_symbol",
        "description": "Propose adding a symbol to the user's watchlist. The user confirms in the app before anything changes.",
        "strict": true,
        "eager_input_streaming": true,
        "input_schema": obj(json!({
            "symbol": text("Provider symbol: BTCUSDT (Binance), EUR/USD or XAU/USD or SPX (Twelve Data), AAPL (Alpaca)"),
            "name": text("Readable name"),
            "asset_class": {"type": "string", "enum": ["crypto", "forex", "index", "commodity", "stock"]},
            "provider": {"type": "string", "enum": ["binance", "twelve_data", "alpaca"]}
        }))
    })
}

/// Web search server tool, in the variant the model supports.
pub fn web_search(model: &str, max_uses: u32) -> Value {
    let modern = [
        "claude-opus-5",
        "claude-sonnet-5-5",
        "claude-fable",
        "claude-mythos",
        "claude-opus-4-6",
        "claude-opus-4-7",
        "claude-opus-4-8",
        "claude-sonnet-4-6",
    ]
    .iter()
    .any(|p| model.starts_with(p));
    let ty = if modern {
        "web_search_20260209"
    } else {
        "web_search_20250305"
    };
    json!({"type": ty, "name": "web_search", "max_uses": max_uses})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_closed(v: &Value) {
        if v["type"] == "object" {
            assert_eq!(v["additionalProperties"], false);
            let props = v["properties"].as_object().unwrap();
            assert_eq!(v["required"].as_array().unwrap().len(), props.len());
            props.values().for_each(check_closed);
        }
        if let Some(items) = v.get("items") {
            check_closed(items);
        }
    }

    #[test]
    fn every_object_is_closed_and_fully_required() {
        for s in [
            market_brief(),
            chart_view(),
            decision(),
            trade_plan(),
            propose_symbol_tool()["input_schema"].clone(),
        ] {
            check_closed(&s);
        }
    }

    #[test]
    fn web_search_variant_follows_model() {
        assert_eq!(
            web_search("claude-opus-5-5", 3)["type"],
            "web_search_20260209"
        );
        assert_eq!(
            web_search("claude-haiku-5-5", 3)["type"],
            "web_search_20250305"
        );
    }
}

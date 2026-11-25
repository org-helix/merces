//! The four-agent pipeline for one symbol, and the paper tracker's price polling.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use merces_core::{
    Action, AgentId, AgentOutput, AgentStatus, Bias, Candle, ChartView, Decision, EngineEvent,
    MarketBrief, RunOutcome, Signal, Stance, SymbolConfig, Timeframe, TradePlan,
};
use merces_llm::{LlmBackend, Request, StreamEvent};
use merces_market::MarketSource;
use merces_ta::TaSummary;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{Ctx, Lang, prompts, schemas};

type Res<T> = Result<T, String>;

pub async fn run<L: LlmBackend, M: MarketSource>(
    ctx: &Ctx<L, M>,
    run_id: &str,
    sym: &SymbolConfig,
) -> RunOutcome {
    if let Err(e) = ctx.store.start_run(run_id, &sym.symbol, Utc::now()) {
        tracing::warn!("run not saved: {e}");
    }
    ctx.emit(EngineEvent::RunStarted {
        run_id: run_id.into(),
        symbol: sym.symbol.clone(),
    });
    for agent in AgentId::PIPELINE {
        ctx.emit(EngineEvent::Status {
            run_id: run_id.into(),
            agent,
            status: AgentStatus::Idle,
        });
    }
    match steps(ctx, run_id, sym).await {
        Ok(o) => o,
        Err(error) => {
            ctx.emit(EngineEvent::Error {
                agent: None,
                message: format!("{} : {error}", sym.symbol),
            });
            RunOutcome::Failed { error }
        }
    }
}

fn status<L: LlmBackend, M: MarketSource>(
    ctx: &Ctx<L, M>,
    run_id: &str,
    agent: AgentId,
    status: AgentStatus,
) {
    ctx.emit(EngineEvent::Status {
        run_id: run_id.into(),
        agent,
        status,
    });
}

fn activity<L: LlmBackend, M: MarketSource>(
    ctx: &Ctx<L, M>,
    run_id: &str,
    agent: AgentId,
    text: String,
) {
    ctx.emit(EngineEvent::Activity {
        run_id: run_id.into(),
        agent,
        text,
    });
}

/// Price with enough significant digits for any market (BTC, EUR/USD, SHIB…).
pub fn fmt_price(p: f64) -> String {
    if !p.is_finite() {
        return "—".into();
    }
    let digits = if p.abs() >= 1000.0 {
        2
    } else if p.abs() >= 1.0 {
        4
    } else {
        (6 - p.abs().log10().floor() as i32).clamp(4, 10) as usize
    };
    format!("{p:.digits$}")
}

fn bias_label(b: Bias, lang: Lang) -> &'static str {
    match (b, lang) {
        (Bias::Bullish, Lang::Fr) => "haussier",
        (Bias::Bearish, Lang::Fr) => "baissier",
        (Bias::Neutral, Lang::Fr) => "neutre",
        (Bias::Bullish, Lang::En) => "bullish",
        (Bias::Bearish, Lang::En) => "bearish",
        (Bias::Neutral, Lang::En) => "neutral",
    }
}

fn change_pct(c: &[Candle], back: usize) -> Option<f64> {
    let n = c.len();
    (n > back).then(|| (c[n - 1].c / c[n - 1 - back].c - 1.0) * 100.0)
}

/// One agent call: streams to the UI, records usage, parses the structured answer.
async fn call<T: DeserializeOwned, L: LlmBackend, M: MarketSource>(
    ctx: &Ctx<L, M>,
    run_id: &str,
    agent: AgentId,
    user: String,
    schema: Value,
    tools: Vec<Value>,
) -> Res<(T, String)> {
    let cfg = ctx.config();
    let settings = cfg.agent(agent);
    status(ctx, run_id, agent, AgentStatus::Working);
    let mut req = Request::new(
        settings.model.clone(),
        prompts::system(agent, cfg.lang, &settings.extra_instructions),
    )
    .user(user);
    req.schema = Some(schema);
    req.effort = settings.effort;
    req.tools = tools;

    let mut reasoning = String::new();
    let sink = ctx.sink.clone();
    let rid = run_id.to_owned();
    let lang = cfg.lang;
    let mut on = |e: StreamEvent| match e {
        StreamEvent::Thinking(text) => {
            reasoning.push_str(&text);
            sink(EngineEvent::Delta {
                run_id: rid.clone(),
                agent,
                text,
                thinking: true,
            });
        }
        StreamEvent::Text(text) => sink(EngineEvent::Delta {
            run_id: rid.clone(),
            agent,
            text,
            thinking: false,
        }),
        StreamEvent::ToolCall { name, input } => {
            let what = input["query"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| input.to_string());
            let label = match (name.as_str(), lang) {
                ("web_search", Lang::Fr) => "Recherche web".to_owned(),
                ("web_search", Lang::En) => "Web search".to_owned(),
                _ => name,
            };
            sink(EngineEvent::Activity {
                run_id: rid.clone(),
                agent,
                text: format!("{label} : {what}"),
            });
        }
        StreamEvent::SearchResults(n) => sink(EngineEvent::Activity {
            run_id: rid.clone(),
            agent,
            text: match lang {
                Lang::Fr => format!("{n} résultats"),
                Lang::En => format!("{n} results"),
            },
        }),
    };
    let result = merces_llm::complete(&*ctx.llm, req, &mut on).await;
    let resp = match result {
        Ok(r) => r,
        Err(e) => {
            status(ctx, run_id, agent, AgentStatus::Error);
            return Err(format!("{} : {e}", agent.key()));
        }
    };
    ctx.record_usage(agent, resp.usage);
    match resp.parse_json::<T>() {
        Ok(v) => Ok((v, reasoning)),
        Err(e) => {
            status(ctx, run_id, agent, AgentStatus::Error);
            Err(format!("{} : {e}", agent.key()))
        }
    }
}

fn publish<L: LlmBackend, M: MarketSource>(
    ctx: &Ctx<L, M>,
    run_id: &str,
    sym: &SymbolConfig,
    agent: AgentId,
    output: AgentOutput,
    reasoning: &str,
) {
    let now = Utc::now();
    if let Err(e) = ctx
        .store
        .add_output(run_id, &sym.symbol, agent, &output, reasoning, now)
    {
        tracing::warn!("output not saved: {e}");
    }
    ctx.emit(EngineEvent::Output {
        run_id: run_id.into(),
        at: now,
        symbol: sym.symbol.clone(),
        agent,
        output,
    });
}

fn hand_off<L: LlmBackend, M: MarketSource>(
    ctx: &Ctx<L, M>,
    run_id: &str,
    sym: &SymbolConfig,
    from: AgentId,
    summary: String,
) {
    let Some(to) = from.next() else { return };
    if let Err(e) = ctx
        .store
        .add_message(run_id, from, to, &summary, Utc::now())
    {
        tracing::warn!("message not saved: {e}");
    }
    ctx.emit(EngineEvent::Message {
        run_id: run_id.into(),
        from,
        to,
        symbol: sym.symbol.clone(),
        summary,
    });
}

async fn steps<L: LlmBackend, M: MarketSource>(
    ctx: &Ctx<L, M>,
    run_id: &str,
    sym: &SymbolConfig,
) -> Res<RunOutcome> {
    let cfg = ctx.config();
    let lang = cfg.lang;
    let limits = cfg.limits_for(sym);
    let now = Utc::now();

    // ── Data ───────────────────────────────────────────────────────────────
    status(ctx, run_id, AgentId::Research, AgentStatus::Working);
    activity(
        ctx,
        run_id,
        AgentId::Research,
        format!("{} : prix, bougies et news…", sym.symbol),
    );
    let (price, news, d1, h4, h1, m15) = tokio::join!(
        ctx.market.last_price(sym),
        ctx.market.news(sym, 12),
        ctx.market.candles(sym, Timeframe::D1, 300),
        ctx.market.candles(sym, Timeframe::H4, 300),
        ctx.market.candles(sym, Timeframe::H1, 300),
        ctx.market.candles(sym, Timeframe::M15, 300),
    );
    let price = price.map_err(|e| format!("prix : {e}"))?;
    ctx.emit(EngineEvent::Price {
        symbol: sym.symbol.clone(),
        price,
    });
    let candles = [
        (Timeframe::D1, d1.map_err(|e| format!("bougies 1D : {e}"))?),
        (Timeframe::H4, h4.map_err(|e| format!("bougies 4h : {e}"))?),
        (Timeframe::H1, h1.map_err(|e| format!("bougies 1h : {e}"))?),
        (
            Timeframe::M15,
            m15.map_err(|e| format!("bougies 15m : {e}"))?,
        ),
    ];
    let summaries: Vec<TaSummary> = candles
        .iter()
        .filter_map(|(tf, c)| merces_ta::summarize(*tf, c))
        .collect();
    if summaries.is_empty() {
        return Err("aucune bougie reçue".into());
    }
    let daily = &candles[0].1;
    let news = news.unwrap_or_default();
    let header = format!(
        "Symbol: {} ({}), asset class: {:?}, data: {}.\nNow (UTC): {} ({}).\nLast price: {}.",
        sym.symbol,
        sym.name,
        sym.class,
        sym.provider.label(),
        now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        now.format("%A"),
        fmt_price(price),
    );

    // ── Agent 1: research ──────────────────────────────────────────────────
    let mut user = format!(
        "{header}\nChange 1 day: {}, 5 days: {}, 20 days: {}.\n",
        change_pct(daily, 1).map_or("n/a".into(), |v| format!("{v:+.2}%")),
        change_pct(daily, 5).map_or("n/a".into(), |v| format!("{v:+.2}%")),
        change_pct(daily, 20).map_or("n/a".into(), |v| format!("{v:+.2}%")),
    );
    if !news.is_empty() {
        user.push_str("\nRecent headlines (Finnhub):\n");
        for n in &news {
            user.push_str(&format!(
                "- [{}] {} ({}) {}\n",
                n.datetime.format("%m-%d %H:%M"),
                n.headline,
                n.source,
                n.url
            ));
        }
    }
    user.push_str("\nTask: search today's news and economic calendar relevant to this symbol, then write the market brief.");
    let model = cfg.agent(AgentId::Research).model;
    let tools = vec![schemas::web_search(&model, cfg.research_searches.max(1))];
    let (brief, why): (MarketBrief, String) = call(
        ctx,
        run_id,
        AgentId::Research,
        user,
        schemas::market_brief(),
        tools,
    )
    .await?;
    publish(
        ctx,
        run_id,
        sym,
        AgentId::Research,
        AgentOutput::Brief(brief.clone()),
        &why,
    );
    status(ctx, run_id, AgentId::Research, AgentStatus::Done);
    hand_off(
        ctx,
        run_id,
        sym,
        AgentId::Research,
        format!(
            "{} {} · {} {} · sentiment {:+}",
            if lang == Lang::Fr { "Jour" } else { "Day" },
            bias_label(brief.bias_day, lang),
            if lang == Lang::Fr { "semaine" } else { "week" },
            bias_label(brief.bias_week, lang),
            brief.sentiment
        ),
    );

    // ── Agent 2: charts ────────────────────────────────────────────────────
    let user = format!(
        "{header}\n\nResearch brief (agent 1):\n{}\n\nIndicator summaries computed from real candles (UTC), \
         higher timeframe first:\n{}\n\nTask: confirm or challenge the research bias from the charts and list the key levels.",
        serde_json::to_string_pretty(&brief).unwrap_or_default(),
        serde_json::to_string_pretty(&summaries).unwrap_or_default(),
    );
    let (chart, why): (ChartView, String) = call(
        ctx,
        run_id,
        AgentId::Chart,
        user,
        schemas::chart_view(),
        vec![],
    )
    .await?;
    publish(
        ctx,
        run_id,
        sym,
        AgentId::Chart,
        AgentOutput::Chart(chart.clone()),
        &why,
    );
    status(ctx, run_id, AgentId::Chart, AgentStatus::Done);
    let stance = match (chart.stance, lang) {
        (Stance::Confirms, Lang::Fr) => "confirme",
        (Stance::Contradicts, Lang::Fr) => "contredit",
        (Stance::Mixed, Lang::Fr) => "mitigé",
        (Stance::Confirms, Lang::En) => "confirms",
        (Stance::Contradicts, Lang::En) => "contradicts",
        (Stance::Mixed, Lang::En) => "mixed",
    };
    hand_off(
        ctx,
        run_id,
        sym,
        AgentId::Chart,
        format!(
            "{stance} · {} · {} {}%",
            bias_label(chart.bias, lang),
            if lang == Lang::Fr {
                "confiance"
            } else {
                "confidence"
            },
            chart.confidence
        ),
    );

    // ── Agent 3: decision ──────────────────────────────────────────────────
    let user = format!(
        "{header}\n\nResearch brief (agent 1):\n{}\n\nChart view (agent 2):\n{}\n\nRisk limits: minimum reward/risk {:.2} \
         to TP1, at most {:.2}% of the account at risk per trade.\n\nTask: decide buy, sell or no_trade for today.",
        serde_json::to_string_pretty(&brief).unwrap_or_default(),
        serde_json::to_string_pretty(&chart).unwrap_or_default(),
        limits.min_rr,
        limits.max_risk_pct,
    );
    let (decision, why): (Decision, String) = call(
        ctx,
        run_id,
        AgentId::Decision,
        user,
        schemas::decision(),
        vec![],
    )
    .await?;
    publish(
        ctx,
        run_id,
        sym,
        AgentId::Decision,
        AgentOutput::Decision(decision.clone()),
        &why,
    );
    if decision.action == Action::NoTrade {
        status(ctx, run_id, AgentId::Decision, AgentStatus::NoTrade);
        return Ok(RunOutcome::NoTrade {
            reason: decision.rationale,
        });
    }
    status(ctx, run_id, AgentId::Decision, AgentStatus::Done);
    hand_off(
        ctx,
        run_id,
        sym,
        AgentId::Decision,
        format!(
            "{} · {} {}/100",
            match (decision.action, lang) {
                (Action::Buy, Lang::Fr) => "ACHAT",
                (Action::Sell, Lang::Fr) => "VENTE",
                (Action::Buy, Lang::En) => "BUY",
                _ => "SELL",
            },
            if lang == Lang::Fr { "risque" } else { "risk" },
            decision.risk_score
        ),
    );

    // ── Agent 4: execution plan ────────────────────────────────────────────
    let atr_ref = summaries
        .iter()
        .find(|s| s.timeframe == Timeframe::H4.label())
        .or_else(|| {
            summaries
                .iter()
                .find(|s| s.timeframe == Timeframe::H1.label())
        })
        .and_then(|s| s.atr14)
        .unwrap_or(chart.atr);
    let near: Vec<_> = summaries
        .iter()
        .filter(|s| s.timeframe == "4h" || s.timeframe == "1h" || s.timeframe == "15m")
        .map(|s| serde_json::json!({"timeframe": s.timeframe, "supports": s.supports, "resistances": s.resistances,
            "atr14": s.atr14, "ema20": s.ema20, "vwap_recent": s.vwap_recent}))
        .collect();
    let base = format!(
        "{header}\n\nDecision (agent 3):\n{}\n\nChart view (agent 2):\n{}\n\nNearby levels:\n{}\n\nReference ATR (4h, 14): {}.\n\
         Limits enforced by software: reward/risk to TP1 ≥ {:.2}; risk ≤ {:.2}% of the account; entry within {:.1} ATR of the \
         current price (between {} and {}); the entry window must start at or after now and end after it starts.\n\n\
         Task: build the {} trade plan.",
        serde_json::to_string_pretty(&decision).unwrap_or_default(),
        serde_json::to_string_pretty(&chart).unwrap_or_default(),
        serde_json::to_string_pretty(&near).unwrap_or_default(),
        fmt_price(atr_ref),
        limits.min_rr,
        limits.max_risk_pct,
        limits.max_entry_atr,
        fmt_price(price - limits.max_entry_atr * atr_ref),
        fmt_price(price + limits.max_entry_atr * atr_ref),
        if decision.action == Action::Buy {
            "long"
        } else {
            "short"
        },
    );
    let mut user = base.clone();
    for attempt in 0..2 {
        let (plan, why): (TradePlan, String) = call(
            ctx,
            run_id,
            AgentId::Execution,
            user.clone(),
            schemas::trade_plan(),
            vec![],
        )
        .await?;
        match plan.validate(price, atr_ref, &limits) {
            Ok(()) => {
                publish(
                    ctx,
                    run_id,
                    sym,
                    AgentId::Execution,
                    AgentOutput::Plan(plan.clone()),
                    &why,
                );
                status(ctx, run_id, AgentId::Execution, AgentStatus::Done);
                let mut signal = Signal::new(
                    format!("{run_id}-sig"),
                    run_id.into(),
                    sym.symbol.clone(),
                    plan,
                    price,
                    Utc::now(),
                );
                if signal.plan.confidence < limits.min_confidence {
                    signal.flagged.push(format!(
                        "confiance {} % sous le seuil de {} %",
                        signal.plan.confidence, limits.min_confidence
                    ));
                }
                if let Err(e) = ctx.store.upsert_signal(&signal) {
                    tracing::warn!("signal not saved: {e}");
                }
                let signal_id = signal.id.clone();
                ctx.emit(EngineEvent::Signal(signal));
                return Ok(RunOutcome::Trade { signal_id });
            }
            Err(errors) if attempt == 0 => {
                activity(
                    ctx,
                    run_id,
                    AgentId::Execution,
                    format!("Plan refusé par les garde-fous : {}", errors.join(" ; ")),
                );
                user = format!(
                    "{base}\n\nYour previous plan was rejected by the risk software:\n{}\nPrevious plan:\n{}\n\nFix the plan so it \
                     respects every limit. If no sound plan can, keep the side but say so clearly in the notes.",
                    errors
                        .iter()
                        .map(|e| format!("- {e}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    serde_json::to_string_pretty(&plan).unwrap_or_default(),
                );
            }
            Err(errors) => {
                publish(
                    ctx,
                    run_id,
                    sym,
                    AgentId::Execution,
                    AgentOutput::Plan(plan),
                    &why,
                );
                status(ctx, run_id, AgentId::Execution, AgentStatus::Error);
                return Ok(RunOutcome::Invalid { errors });
            }
        }
    }
    unreachable!("the loop returns on its second attempt")
}

/// Poll prices for open signals and the focused symbol; advance the paper tracker.
pub async fn poll_prices<L: LlmBackend, M: MarketSource>(ctx: &Ctx<L, M>, focus: Option<&str>) {
    let mut open = ctx.store.signals(true, 200).unwrap_or_default();
    let mut symbols: BTreeSet<String> = open.iter().map(|s| s.symbol.clone()).collect();
    if let Some(f) = focus {
        symbols.insert(f.to_owned());
    }
    let cfg = ctx.config();
    for symbol in symbols {
        let Some(sc) = cfg.symbol(&symbol) else {
            continue;
        };
        match ctx.market.last_price(sc).await {
            Ok(price) => {
                ctx.emit(EngineEvent::Price {
                    symbol: symbol.clone(),
                    price,
                });
                let now: DateTime<Utc> = Utc::now();
                for s in open.iter_mut().filter(|s| s.symbol == symbol) {
                    if s.update(price, now) {
                        if let Err(e) = ctx.store.upsert_signal(s) {
                            tracing::warn!("signal not saved: {e}");
                        }
                        ctx.emit(EngineEvent::Signal(s.clone()));
                    }
                }
            }
            Err(e) => tracing::debug!("price {symbol}: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_keep_meaningful_digits() {
        assert_eq!(fmt_price(64250.123), "64250.12");
        assert_eq!(fmt_price(1.08456), "1.0846");
        assert_eq!(fmt_price(0.00001234), "0.0000123400");
    }
}

//! End-to-end pipeline tests with a scripted LLM, the demo market and an
//! in-memory database.

use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use chrono::Utc;
use merces_agents::demo::{DemoLlm, DemoMarket};
use merces_agents::{Command, Ctx, EngineConfig};
use merces_core::{AgentId, AgentStatus, EngineEvent, RunOutcome};
use merces_llm::{LlmBackend, Request, Response, StreamEvent};
use merces_store::Store;
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// Answers each agent with a fixed JSON; the decision is configurable.
struct Scripted {
    action: &'static str,
}

impl LlmBackend for Scripted {
    async fn send(
        &self,
        req: &Request,
        on: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> merces_llm::Result<Response> {
        let agent = req
            .system
            .lines()
            .next()
            .unwrap()
            .trim_start_matches("Agent: ")
            .to_owned();
        let user = req.messages.last().unwrap()["content"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let price: f64 = user
            .split("Last price: ")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .map(|s| s.trim_end_matches('.').parse().unwrap())
            .unwrap_or(100.0);
        on(StreamEvent::Thinking(format!("thinking as {agent}")));
        let now = Utc::now();
        let answer: Value = match agent.as_str() {
            "research" => {
                json!({"bias_day": "bullish", "bias_week": "neutral", "sentiment": 40, "summary": "s",
                "catalysts": [], "risks": [], "sources": []})
            }
            "chart" => {
                json!({"stance": "confirms", "bias": "bullish", "structure": [], "levels": [],
                "atr": price * 0.01, "summary": "s", "confidence": 70})
            }
            "decision" => {
                json!({"action": self.action, "rationale": "r", "risk_assessment": "a", "risk_score": 40,
                "horizon": "h", "prediction": "p"})
            }
            "execution" => json!({"side": "long", "entry": price, "stop_loss": price * 0.995,
                "take_profits": [price * 1.01], "entry_window_start": now.to_rfc3339(),
                "entry_window_end": (now + chrono::Duration::hours(4)).to_rfc3339(),
                "risk_reward": 2.0, "risk_pct": 0.5, "confidence": 72, "notes": "n"}),
            other => panic!("unexpected agent {other}"),
        };
        Ok(Response {
            content: vec![json!({"type": "text", "text": answer.to_string()})],
            stop_reason: "end_turn".into(),
            model: req.model.clone(),
            usage: merces_core::Usage {
                input_tokens: 100,
                output_tokens: 50,
                ..Default::default()
            },
        })
    }
}

async fn run_once<L: LlmBackend>(llm: L) -> (Vec<EngineEvent>, Arc<Store>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let store = Arc::new(Store::in_memory().unwrap());
    let ctx = Ctx {
        llm: Arc::new(llm),
        market: Arc::new(DemoMarket),
        store: store.clone(),
        sink: Arc::new(move |e| ev.lock().unwrap().push(e)),
        cfg: Arc::new(RwLock::new(EngineConfig::default())),
    };
    let (tx, rx) = mpsc::unbounded_channel();
    let engine = tokio::spawn(merces_agents::run(ctx, rx));
    tx.send(Command::Run("BTCUSDT".into())).unwrap();
    for _ in 0..400 {
        tokio::time::sleep(Duration::from_millis(25)).await;
        if events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, EngineEvent::RunFinished { .. }))
        {
            break;
        }
    }
    tx.send(Command::Shutdown).unwrap();
    engine.await.unwrap();
    let out = events.lock().unwrap().clone();
    (out, store)
}

fn handoffs(events: &[EngineEvent]) -> Vec<(AgentId, AgentId)> {
    events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Message { from, to, .. } => Some((*from, *to)),
            _ => None,
        })
        .collect()
}

fn outcome(events: &[EngineEvent]) -> RunOutcome {
    events
        .iter()
        .find_map(|e| match e {
            EngineEvent::RunFinished { outcome, .. } => Some(outcome.clone()),
            _ => None,
        })
        .expect("run finished")
}

#[tokio::test]
async fn buy_goes_through_all_four_agents_and_tracks_a_signal() {
    let (events, store) = run_once(Scripted { action: "buy" }).await;
    assert_eq!(
        handoffs(&events),
        vec![
            (AgentId::Research, AgentId::Chart),
            (AgentId::Chart, AgentId::Decision),
            (AgentId::Decision, AgentId::Execution)
        ]
    );
    assert!(
        matches!(outcome(&events), RunOutcome::Trade { .. }),
        "{:?}",
        outcome(&events)
    );
    assert_eq!(store.signals(false, 10).unwrap().len(), 1);
    assert_eq!(
        store
            .outputs_since(Utc::now() - chrono::Duration::hours(1))
            .unwrap()
            .len(),
        4
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Delta { thinking: true, .. }))
    );
    let runs = store.recent_runs(1).unwrap();
    assert!(matches!(runs[0].outcome, Some(RunOutcome::Trade { .. })));
    let usage: u64 = store
        .usage_for_day(Utc::now().date_naive())
        .unwrap()
        .iter()
        .map(|(_, u)| u.input_tokens)
        .sum();
    assert_eq!(usage, 400);
}

#[tokio::test]
async fn no_trade_stops_at_the_decision_agent() {
    let (events, store) = run_once(Scripted { action: "no_trade" }).await;
    assert_eq!(handoffs(&events).len(), 2);
    assert!(matches!(outcome(&events), RunOutcome::NoTrade { .. }));
    assert!(events.iter().any(|e| matches!(
        e,
        EngineEvent::Status {
            agent: AgentId::Decision,
            status: AgentStatus::NoTrade,
            ..
        }
    )));
    assert!(!events.iter().any(|e| matches!(
        e,
        EngineEvent::Status {
            agent: AgentId::Execution,
            status: AgentStatus::Working,
            ..
        }
    )));
    assert!(store.signals(false, 10).unwrap().is_empty());
}

#[tokio::test]
async fn demo_backend_completes_a_run() {
    let (events, _) = run_once(DemoLlm {
        pace: Duration::ZERO,
    })
    .await;
    match outcome(&events) {
        RunOutcome::Trade { .. } | RunOutcome::NoTrade { .. } => {}
        other => panic!("demo run ended with {other:?}"),
    }
}

//! The Merces engine: runs the four-agent pipeline per symbol, the daily scheduler,
//! the paper tracker and the assistant. It talks to the UI only through
//! [`EngineEvent`]s sent to a sink and [`Command`]s received on a channel.

mod assistant;
pub mod demo;
mod pipeline;
mod prompts;
pub mod schemas;

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use merces_core::{
    AgentId, AgentSettings, EngineEvent, RiskLimits, RunOutcome, SymbolConfig, Usage,
};
use merces_llm::LlmBackend;
use merces_market::MarketSource;
use merces_store::Store;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub use assistant::ChatTurn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    #[default]
    Fr,
    En,
}

/// What the engine needs from the app configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineConfig {
    pub agents: BTreeMap<AgentId, AgentSettings>,
    pub risk: RiskLimits,
    pub symbols: Vec<SymbolConfig>,
    pub lang: Lang,
    /// Runs are refused once today's spend reaches this (0 = no limit).
    pub daily_budget_usd: f64,
    /// Web searches allowed per research run.
    pub research_searches: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            agents: AgentId::ALL
                .into_iter()
                .map(|a| (a, AgentSettings::for_agent(a)))
                .collect(),
            risk: RiskLimits::default(),
            symbols: vec![SymbolConfig::default()],
            lang: Lang::Fr,
            daily_budget_usd: 5.0,
            research_searches: 6,
        }
    }
}

impl EngineConfig {
    pub fn agent(&self, a: AgentId) -> AgentSettings {
        self.agents
            .get(&a)
            .cloned()
            .unwrap_or_else(|| AgentSettings::for_agent(a))
    }

    pub fn symbol(&self, symbol: &str) -> Option<&SymbolConfig> {
        self.symbols.iter().find(|s| s.symbol == symbol)
    }

    pub fn limits_for(&self, s: &SymbolConfig) -> RiskLimits {
        s.risk.unwrap_or(self.risk)
    }
}

#[derive(Debug, Clone)]
pub enum Command {
    /// Run the pipeline for one symbol now.
    Run(String),
    RunAll,
    /// Stop the run of a symbol (or every run with `None`).
    Cancel(Option<String>),
    /// Ask the assistant; the full conversation is sent each time.
    Ask(Vec<ChatTurn>),
    Configure(EngineConfig),
    /// The symbol shown in the UI: its price is polled.
    Focus(String),
    Shutdown,
}

pub type Sink = Arc<dyn Fn(EngineEvent) + Send + Sync>;

/// Shared, cheap-to-clone handles used by every task.
pub struct Ctx<L, M> {
    pub llm: Arc<L>,
    pub market: Arc<M>,
    pub store: Arc<Store>,
    pub sink: Sink,
    pub cfg: Arc<RwLock<EngineConfig>>,
}

impl<L, M> Clone for Ctx<L, M> {
    fn clone(&self) -> Self {
        Self {
            llm: self.llm.clone(),
            market: self.market.clone(),
            store: self.store.clone(),
            sink: self.sink.clone(),
            cfg: self.cfg.clone(),
        }
    }
}

impl<L: LlmBackend, M: MarketSource> Ctx<L, M> {
    pub fn emit(&self, e: EngineEvent) {
        (self.sink)(e);
    }

    pub fn config(&self) -> EngineConfig {
        self.cfg.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn record_usage(&self, agent: AgentId, usage: Usage) {
        if let Err(e) = self.store.add_usage(Utc::now().date_naive(), agent, &usage) {
            tracing::warn!("usage not saved: {e}");
        }
        self.emit(EngineEvent::Usage { agent, usage });
    }

    pub fn spent_today(&self) -> f64 {
        self.store
            .usage_for_day(Utc::now().date_naive())
            .map(|v| v.iter().map(|(_, u)| u.cost_usd).sum())
            .unwrap_or(0.0)
    }
}

/// Run the engine until [`Command::Shutdown`] or the channel closes.
pub async fn run<L: LlmBackend, M: MarketSource>(
    ctx: Ctx<L, M>,
    mut commands: mpsc::UnboundedReceiver<Command>,
) {
    replay_today(&ctx);
    let mut runs: HashMap<String, JoinHandle<()>> = HashMap::new();
    let mut assistant: Option<JoinHandle<()>> = None;
    let mut focus: Option<String> = None;
    let mut schedule_tick = tokio::time::interval(Duration::from_secs(30));
    let mut price_tick = tokio::time::interval(Duration::from_secs(15));
    schedule_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    price_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        runs.retain(|_, h| !h.is_finished());
        tokio::select! {
            cmd = commands.recv() => {
                let Some(cmd) = cmd else { break };
                match cmd {
                    Command::Run(symbol) => start_run(&ctx, &mut runs, &symbol),
                    Command::RunAll => {
                        let symbols: Vec<String> = ctx.config().symbols.iter()
                            .filter(|s| s.enabled).map(|s| s.symbol.clone()).collect();
                        for s in symbols { start_run(&ctx, &mut runs, &s) }
                    }
                    Command::Cancel(which) => {
                        for (symbol, h) in runs.iter() {
                            if which.as_ref().is_none_or(|w| w == symbol) {
                                h.abort();
                                ctx.emit(EngineEvent::Info(format!("{symbol} : analyse annulée")));
                            }
                        }
                        if which.is_none()
                            && let Some(h) = assistant.take() { h.abort() }
                        reset_statuses(&ctx);
                    }
                    Command::Ask(history) => {
                        if let Some(h) = assistant.take() { h.abort() }
                        let c = ctx.clone();
                        assistant = Some(tokio::spawn(async move { assistant::answer(&c, history).await }));
                    }
                    Command::Configure(cfg) => {
                        *ctx.cfg.write().unwrap_or_else(|e| e.into_inner()) = cfg;
                    }
                    Command::Focus(symbol) => {
                        focus = Some(symbol);
                        price_tick.reset_immediately();
                    }
                    Command::Shutdown => break,
                }
            }
            _ = schedule_tick.tick() => scheduled_runs(&ctx, &mut runs, Utc::now()),
            _ = price_tick.tick() => {
                let c = ctx.clone();
                let f = focus.clone();
                tokio::spawn(async move { pipeline::poll_prices(&c, f.as_deref()).await });
            }
        }
    }
    for h in runs.values() {
        h.abort();
    }
}

fn start_run<L: LlmBackend, M: MarketSource>(
    ctx: &Ctx<L, M>,
    runs: &mut HashMap<String, JoinHandle<()>>,
    symbol: &str,
) {
    if runs.contains_key(symbol) {
        ctx.emit(EngineEvent::Info(format!(
            "{symbol} : une analyse est déjà en cours"
        )));
        return;
    }
    let cfg = ctx.config();
    let Some(sym) = cfg.symbol(symbol).cloned() else {
        ctx.emit(EngineEvent::Error {
            agent: None,
            message: format!("symbole inconnu : {symbol}"),
        });
        return;
    };
    if cfg.daily_budget_usd > 0.0 && ctx.spent_today() >= cfg.daily_budget_usd {
        ctx.emit(EngineEvent::Error {
            agent: None,
            message: format!(
                "Budget du jour atteint ({:.2} $). Augmentez-le dans Réglages → Agents.",
                cfg.daily_budget_usd
            ),
        });
        return;
    }
    let c = ctx.clone();
    let handle = tokio::spawn(async move {
        let run_id = format!(
            "{}-{}",
            Utc::now().format("%Y%m%d-%H%M%S"),
            sanitize(&sym.symbol)
        );
        let outcome = pipeline::run(&c, &run_id, &sym).await;
        if let Err(e) = c.store.finish_run(&run_id, &outcome, Utc::now()) {
            tracing::warn!("run not saved: {e}");
        }
        c.emit(EngineEvent::RunFinished {
            run_id,
            symbol: sym.symbol.clone(),
            outcome,
        });
    });
    runs.insert(symbol.to_owned(), handle);
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Start every symbol whose daily time has passed and which has not run since.
fn scheduled_runs<L: LlmBackend, M: MarketSource>(
    ctx: &Ctx<L, M>,
    runs: &mut HashMap<String, JoinHandle<()>>,
    now: DateTime<Utc>,
) {
    for s in ctx.config().symbols.iter().filter(|s| s.enabled) {
        let Some(at) = s.schedule_utc else { continue };
        let due = now.date_naive().and_time(at).and_utc();
        if now < due {
            continue;
        }
        let last = ctx.store.last_run_at(&s.symbol).ok().flatten();
        if last.is_none_or(|l| l < due) && !runs.contains_key(&s.symbol) {
            ctx.emit(EngineEvent::Info(format!(
                "{} : analyse planifiée",
                s.symbol
            )));
            start_run(ctx, runs, &s.symbol);
        }
    }
}

fn reset_statuses<L: LlmBackend, M: MarketSource>(ctx: &Ctx<L, M>) {
    for agent in AgentId::PIPELINE {
        ctx.emit(EngineEvent::Status {
            run_id: String::new(),
            agent,
            status: merces_core::AgentStatus::Idle,
        });
    }
}

/// Send today's results, usage and the recent signals so a restarted UI is complete.
fn replay_today<L: LlmBackend, M: MarketSource>(ctx: &Ctx<L, M>) {
    let midnight = Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap_or_default()
        .and_utc();
    if let Ok(mut outputs) = ctx.store.outputs_since(midnight) {
        outputs.reverse();
        for o in outputs {
            ctx.emit(EngineEvent::Output {
                run_id: o.run_id,
                at: o.created_at,
                symbol: o.symbol,
                agent: o.agent,
                output: o.output,
            });
        }
    }
    if let Ok(usage) = ctx.store.usage_for_day(Utc::now().date_naive()) {
        for (agent, usage) in usage {
            ctx.emit(EngineEvent::Usage { agent, usage });
        }
    }
    if let Ok(mut signals) = ctx.store.signals(false, 200) {
        signals.reverse();
        for s in signals {
            ctx.emit(EngineEvent::Signal(s));
        }
    }
    if let Ok(runs) = ctx.store.recent_runs(1)
        && let Some(r) = runs.into_iter().next()
        && let Some(outcome) = r.outcome
    {
        ctx.emit(EngineEvent::RunFinished {
            run_id: r.id,
            symbol: r.symbol,
            outcome,
        });
    }
}

/// Convenience for the outcome text shown in the UI.
pub fn outcome_label(o: &RunOutcome, lang: Lang) -> String {
    match (o, lang) {
        (RunOutcome::Trade { .. }, Lang::Fr) => "Plan de trade émis".into(),
        (RunOutcome::Trade { .. }, Lang::En) => "Trade plan issued".into(),
        (RunOutcome::NoTrade { .. }, Lang::Fr) => "Ne rien faire aujourd'hui".into(),
        (RunOutcome::NoTrade { .. }, Lang::En) => "No trade today".into(),
        (RunOutcome::Invalid { .. }, Lang::Fr) => "Plan rejeté par les garde-fous".into(),
        (RunOutcome::Invalid { .. }, Lang::En) => "Plan rejected by the guardrails".into(),
        (RunOutcome::Failed { error }, _) => format!("Erreur : {error}"),
        (RunOutcome::Cancelled, Lang::Fr) => "Annulé".into(),
        (RunOutcome::Cancelled, Lang::En) => "Cancelled".into(),
    }
}

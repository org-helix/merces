//! UI-side view of what the engine reports. Updated from [`EngineEvent`]s once
//! per frame; read by the 3D scene and the panels.

use std::collections::{BTreeMap, HashMap, VecDeque};

use bevy::prelude::Resource;
use chrono::{DateTime, Local, Utc};
use merces_agents::ChatTurn;
use merces_core::{
    AgentId, AgentOutput, AgentStatus, EngineEvent, RunOutcome, Signal, SymbolConfig, Usage,
};

const LIVE_CAP: usize = 24_000;

#[derive(Debug, Clone, Default)]
pub struct AgentView {
    pub status: AgentStatus,
    pub run_id: String,
    /// Reasoning summary streamed by Claude during the current run.
    pub thinking: String,
    /// Raw answer (JSON) streamed during the current run.
    pub answer: String,
    pub activity: Vec<(DateTime<Utc>, String)>,
    /// When the status last changed (drives animations).
    pub since: f64,
}

#[derive(Debug, Clone)]
pub struct OutputRecord {
    pub at: DateTime<Utc>,
    pub run_id: String,
    pub agent: AgentId,
    pub output: AgentOutput,
}

#[derive(Debug, Clone)]
pub struct MessageRecord {
    pub at: DateTime<Utc>,
    pub from: AgentId,
    pub to: AgentId,
    pub summary: String,
}

/// Everything about one symbol.
#[derive(Debug, Clone, Default)]
pub struct SymbolView {
    pub agents: [AgentView; 4],
    pub outputs: Vec<OutputRecord>,
    pub messages: Vec<MessageRecord>,
    pub running: Option<String>,
    pub last_outcome: Option<(DateTime<Utc>, RunOutcome)>,
}

impl SymbolView {
    pub fn agent(&self, a: AgentId) -> &AgentView {
        &self.agents[a.index().min(3)]
    }

    /// Today's outputs (local day) of one agent, newest first.
    pub fn today(&self, a: AgentId) -> impl Iterator<Item = &OutputRecord> {
        let today = Local::now().date_naive();
        self.outputs
            .iter()
            .rev()
            .filter(move |o| o.agent == a && o.at.with_timezone(&Local).date_naive() == today)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
    Success,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub kind: ToastKind,
    pub text: String,
    pub until: f64,
}

/// A message to animate between two planets.
#[derive(Debug, Clone, Copy)]
pub struct FlowRequest {
    pub from: AgentId,
    pub to: AgentId,
}

#[derive(Resource, Default)]
pub struct Hub {
    pub symbols: HashMap<String, SymbolView>,
    run_symbol: HashMap<String, String>,
    pub usage: BTreeMap<AgentId, Usage>,
    pub prices: HashMap<String, f64>,
    pub signals: BTreeMap<String, Signal>,
    pub chat: Vec<ChatTurn>,
    pub chat_busy: bool,
    pub proposals: Vec<SymbolConfig>,
    pub toasts: VecDeque<Toast>,
    pub flows: Vec<FlowRequest>,
    /// Engine time (seconds since start) used for toasts and animations.
    pub now: f64,
}

impl Hub {
    pub fn symbol(&self, s: &str) -> Option<&SymbolView> {
        self.symbols.get(s)
    }

    pub fn spent_today(&self) -> f64 {
        self.usage.values().map(|u| u.cost_usd).sum()
    }

    pub fn running(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .symbols
            .iter()
            .filter(|(_, s)| s.running.is_some())
            .map(|(k, _)| k.clone())
            .collect();
        v.sort();
        v
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        let secs = if kind == ToastKind::Error { 9.0 } else { 5.0 };
        self.toasts.push_back(Toast {
            kind,
            text: text.into(),
            until: self.now + secs,
        });
        while self.toasts.len() > 4 {
            self.toasts.pop_front();
        }
    }

    fn sym_of_run(&mut self, run_id: &str) -> Option<&mut SymbolView> {
        let s = self.run_symbol.get(run_id)?.clone();
        Some(self.symbols.entry(s).or_default())
    }

    /// Apply one engine event. `active` is the symbol shown in the 3D view.
    pub fn apply(&mut self, e: EngineEvent, active: &str) {
        let now = self.now;
        match e {
            EngineEvent::RunStarted { run_id, symbol } => {
                self.run_symbol.insert(run_id.clone(), symbol.clone());
                let v = self.symbols.entry(symbol).or_default();
                v.running = Some(run_id.clone());
                for a in v.agents.iter_mut() {
                    *a = AgentView {
                        run_id: run_id.clone(),
                        since: now,
                        ..Default::default()
                    };
                }
            }
            EngineEvent::Status {
                run_id,
                agent,
                status,
            } => {
                if run_id.is_empty() {
                    // Global reset (cancel).
                    for v in self.symbols.values_mut() {
                        v.agents[agent.index().min(3)].status = status;
                        v.running = None;
                    }
                } else if let Some(v) = self.sym_of_run(&run_id) {
                    let a = &mut v.agents[agent.index().min(3)];
                    if a.status != status {
                        a.since = now;
                    }
                    a.status = status;
                }
            }
            EngineEvent::Delta {
                run_id,
                agent,
                text,
                thinking,
            } => {
                if let Some(v) = self.sym_of_run(&run_id) {
                    let a = &mut v.agents[agent.index().min(3)];
                    let buf = if thinking {
                        &mut a.thinking
                    } else {
                        &mut a.answer
                    };
                    buf.push_str(&text);
                    if buf.len() > LIVE_CAP {
                        let cut = buf.len() - LIVE_CAP;
                        let cut = (cut..buf.len())
                            .find(|i| buf.is_char_boundary(*i))
                            .unwrap_or(0);
                        buf.drain(..cut);
                    }
                }
            }
            EngineEvent::Activity {
                run_id,
                agent,
                text,
            } => {
                if let Some(v) = self.sym_of_run(&run_id) {
                    v.agents[agent.index().min(3)]
                        .activity
                        .push((Utc::now(), text));
                }
            }
            EngineEvent::Message {
                run_id,
                from,
                to,
                symbol,
                summary,
            } => {
                self.run_symbol
                    .entry(run_id)
                    .or_insert_with(|| symbol.clone());
                if symbol == active {
                    self.flows.push(FlowRequest { from, to });
                }
                self.symbols
                    .entry(symbol)
                    .or_default()
                    .messages
                    .push(MessageRecord {
                        at: Utc::now(),
                        from,
                        to,
                        summary,
                    });
            }
            EngineEvent::Output {
                run_id,
                at,
                symbol,
                agent,
                output,
            } => {
                self.run_symbol
                    .entry(run_id.clone())
                    .or_insert_with(|| symbol.clone());
                self.symbols
                    .entry(symbol)
                    .or_default()
                    .outputs
                    .push(OutputRecord {
                        at,
                        run_id,
                        agent,
                        output,
                    });
            }
            EngineEvent::Usage { agent, usage } => {
                *self.usage.entry(agent).or_default() += usage;
            }
            EngineEvent::RunFinished {
                run_id,
                symbol,
                outcome,
            } => {
                let v = self.symbols.entry(symbol.clone()).or_default();
                if v.running.as_deref() == Some(run_id.as_str()) {
                    v.running = None;
                }
                let first_load = v.last_outcome.is_none()
                    && v.running.is_none()
                    && v.agents[0].run_id.is_empty();
                v.last_outcome = Some((Utc::now(), outcome.clone()));
                if !first_load {
                    let (kind, text) = match &outcome {
                        RunOutcome::Trade { .. } => {
                            (ToastKind::Success, format!("{symbol} : plan de trade émis"))
                        }
                        RunOutcome::NoTrade { .. } => (
                            ToastKind::Info,
                            format!("{symbol} : ne rien faire aujourd'hui"),
                        ),
                        RunOutcome::Invalid { .. } => (
                            ToastKind::Error,
                            format!("{symbol} : plan rejeté par les garde-fous"),
                        ),
                        RunOutcome::Failed { error } => {
                            (ToastKind::Error, format!("{symbol} : {error}"))
                        }
                        RunOutcome::Cancelled => (ToastKind::Info, format!("{symbol} : annulé")),
                    };
                    self.toast(kind, text);
                }
            }
            EngineEvent::Price { symbol, price } => {
                self.prices.insert(symbol, price);
            }
            EngineEvent::Signal(s) => {
                self.signals.insert(s.id.clone(), s);
            }
            EngineEvent::AssistantDelta(t) => match self.chat.last_mut() {
                Some(last) if !last.user => last.text.push_str(&t),
                _ => self.chat.push(ChatTurn {
                    user: false,
                    text: t,
                }),
            },
            EngineEvent::AssistantDone => self.chat_busy = false,
            EngineEvent::SymbolProposal(s) => self.proposals.push(s),
            EngineEvent::Error { agent, message } => {
                if agent == Some(AgentId::Assistant) {
                    self.chat_busy = false;
                }
                self.toast(ToastKind::Error, message);
            }
            EngineEvent::Info(text) => self.toast(ToastKind::Info, text),
        }
    }
}

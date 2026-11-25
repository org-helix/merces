use serde::{Deserialize, Serialize};

use crate::{AgentId, AgentOutput, AgentStatus, RunOutcome, Signal, SymbolConfig, Usage};

/// Everything the engine reports to the UI. Sent over a channel, in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EngineEvent {
    RunStarted {
        run_id: String,
        symbol: String,
    },
    Status {
        run_id: String,
        agent: AgentId,
        status: AgentStatus,
    },
    /// Streamed text: the agent's reasoning summary (`thinking`) or its answer.
    Delta {
        run_id: String,
        agent: AgentId,
        text: String,
        thinking: bool,
    },
    /// The agent used a tool (web search query, data fetch…).
    Activity {
        run_id: String,
        agent: AgentId,
        text: String,
    },
    /// One agent handed its work to the next one.
    Message {
        run_id: String,
        from: AgentId,
        to: AgentId,
        symbol: String,
        summary: String,
    },
    Output {
        run_id: String,
        at: chrono::DateTime<chrono::Utc>,
        symbol: String,
        agent: AgentId,
        output: AgentOutput,
    },
    Usage {
        agent: AgentId,
        usage: Usage,
    },
    RunFinished {
        run_id: String,
        symbol: String,
        outcome: RunOutcome,
    },
    Price {
        symbol: String,
        price: f64,
    },
    Signal(Signal),
    AssistantDelta(String),
    AssistantDone,
    /// The assistant suggests adding a symbol; the user confirms in the UI.
    SymbolProposal(SymbolConfig),
    Error {
        agent: Option<AgentId>,
        message: String,
    },
    Info(String),
}

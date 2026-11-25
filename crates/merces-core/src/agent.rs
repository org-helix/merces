use serde::{Deserialize, Serialize};

/// The five agents. The first four form the pipeline; the assistant stands apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentId {
    Research,
    Chart,
    Decision,
    Execution,
    Assistant,
}

impl AgentId {
    pub const ALL: [AgentId; 5] = [
        AgentId::Research,
        AgentId::Chart,
        AgentId::Decision,
        AgentId::Execution,
        AgentId::Assistant,
    ];
    pub const PIPELINE: [AgentId; 4] = [
        AgentId::Research,
        AgentId::Chart,
        AgentId::Decision,
        AgentId::Execution,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The agent this one hands its work to.
    pub fn next(self) -> Option<AgentId> {
        match self {
            AgentId::Research => Some(AgentId::Chart),
            AgentId::Chart => Some(AgentId::Decision),
            AgentId::Decision => Some(AgentId::Execution),
            AgentId::Execution | AgentId::Assistant => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            AgentId::Research => "research",
            AgentId::Chart => "chart",
            AgentId::Decision => "decision",
            AgentId::Execution => "execution",
            AgentId::Assistant => "assistant",
        }
    }

    pub fn from_key(key: &str) -> Option<AgentId> {
        AgentId::ALL.into_iter().find(|a| a.key() == key)
    }

    /// Default Claude model for each agent (editable in the settings).
    pub fn default_model(self) -> &'static str {
        match self {
            AgentId::Research | AgentId::Decision => "claude-opus-5-5",
            AgentId::Chart | AgentId::Execution => "claude-sonnet-5-5",
            AgentId::Assistant => "claude-haiku-5-5",
        }
    }

    pub fn default_effort(self) -> Effort {
        match self {
            AgentId::Research | AgentId::Decision => Effort::High,
            AgentId::Chart | AgentId::Execution => Effort::Medium,
            AgentId::Assistant => Effort::Low,
        }
    }
}

/// Claude `output_config.effort`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    Low,
    #[default]
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    pub const ALL: [Effort; 5] = [
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::Xhigh,
        Effort::Max,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::Xhigh => "xhigh",
            Effort::Max => "max",
        }
    }
}

/// Per-agent settings, editable in the configuration panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    pub model: String,
    pub effort: Effort,
    /// Extra instructions appended to the built-in system prompt.
    pub extra_instructions: String,
    pub enabled: bool,
}

impl AgentSettings {
    pub fn for_agent(agent: AgentId) -> Self {
        Self {
            model: agent.default_model().to_owned(),
            effort: agent.default_effort(),
            extra_instructions: String::new(),
            enabled: true,
        }
    }
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self::for_agent(AgentId::Chart)
    }
}

/// What an agent is doing right now (drives the character's look).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    #[default]
    Idle,
    Working,
    Done,
    /// Decision agent concluded "do nothing today".
    NoTrade,
    Error,
}

/// Token usage and cost of one or several calls.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub web_searches: u64,
    pub cost_usd: f64,
}

impl std::ops::AddAssign for Usage {
    fn add_assign(&mut self, o: Self) {
        self.input_tokens += o.input_tokens;
        self.output_tokens += o.output_tokens;
        self.cache_read_tokens += o.cache_read_tokens;
        self.cache_write_tokens += o.cache_write_tokens;
        self.web_searches += o.web_searches;
        self.cost_usd += o.cost_usd;
    }
}

impl Usage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_read_tokens + self.cache_write_tokens
    }
}

//! SQLite persistence. One connection behind a mutex: every operation is short.

use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, NaiveDate, Utc};
use merces_core::{AgentId, AgentOutput, RunOutcome, Signal, Usage};
use rusqlite::{Connection, OptionalExtension, params};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("serialization: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

pub struct Store {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY,
    symbol TEXT NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    outcome TEXT
);
CREATE INDEX IF NOT EXISTS runs_symbol ON runs(symbol, started_at);
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    from_agent TEXT NOT NULL,
    to_agent TEXT NOT NULL,
    summary TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS outputs (
    id INTEGER PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    symbol TEXT NOT NULL,
    agent TEXT NOT NULL,
    output TEXT NOT NULL,
    reasoning TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS outputs_agent ON outputs(agent, created_at);
CREATE TABLE IF NOT EXISTS signals (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    symbol TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    data TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS signals_state ON signals(state);
CREATE TABLE IF NOT EXISTS usage (
    day TEXT NOT NULL,
    agent TEXT NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    web_searches INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL NOT NULL DEFAULT 0,
    PRIMARY KEY (day, agent)
);
"#;

#[derive(Debug, Clone, PartialEq)]
pub struct StoredOutput {
    pub run_id: String,
    pub symbol: String,
    pub agent: AgentId,
    pub output: AgentOutput,
    pub reasoning: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredRun {
    pub id: String,
    pub symbol: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub outcome: Option<RunOutcome>,
}

fn ts(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_default()
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn start_run(&self, id: &str, symbol: &str, at: DateTime<Utc>) -> Result<()> {
        self.db().execute(
            "INSERT INTO runs (id, symbol, started_at) VALUES (?1, ?2, ?3)",
            params![id, symbol, at.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn finish_run(&self, id: &str, outcome: &RunOutcome, at: DateTime<Utc>) -> Result<()> {
        self.db().execute(
            "UPDATE runs SET finished_at = ?2, outcome = ?3 WHERE id = ?1",
            params![id, at.to_rfc3339(), serde_json::to_string(outcome)?],
        )?;
        Ok(())
    }

    /// Last run start time for a symbol (the scheduler runs each symbol once a day).
    pub fn last_run_at(&self, symbol: &str) -> Result<Option<DateTime<Utc>>> {
        Ok(self
            .db()
            .query_row(
                "SELECT started_at FROM runs WHERE symbol = ?1 ORDER BY started_at DESC LIMIT 1",
                params![symbol],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(ts))
    }

    pub fn recent_runs(&self, limit: usize) -> Result<Vec<StoredRun>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT id, symbol, started_at, finished_at, outcome FROM runs ORDER BY started_at DESC LIMIT ?1",
        )?;
        let rows = st.query_map(params![limit as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, symbol, s, f, o) = row?;
            out.push(StoredRun {
                id,
                symbol,
                started_at: ts(s),
                finished_at: f.map(ts),
                outcome: o.and_then(|o| serde_json::from_str(&o).ok()),
            });
        }
        Ok(out)
    }

    pub fn add_message(
        &self,
        run_id: &str,
        from: AgentId,
        to: AgentId,
        summary: &str,
        at: DateTime<Utc>,
    ) -> Result<()> {
        self.db().execute(
            "INSERT INTO messages (run_id, from_agent, to_agent, summary, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![run_id, from.key(), to.key(), summary, at.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn add_output(
        &self,
        run_id: &str,
        symbol: &str,
        agent: AgentId,
        output: &AgentOutput,
        reasoning: &str,
        at: DateTime<Utc>,
    ) -> Result<()> {
        self.db().execute(
            "INSERT INTO outputs (run_id, symbol, agent, output, reasoning, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![run_id, symbol, agent.key(), serde_json::to_string(output)?, reasoning, at.to_rfc3339()],
        )?;
        Ok(())
    }

    /// Outputs created since `since`, newest first.
    pub fn outputs_since(&self, since: DateTime<Utc>) -> Result<Vec<StoredOutput>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT run_id, symbol, agent, output, reasoning, created_at FROM outputs
             WHERE created_at >= ?1 ORDER BY created_at DESC",
        )?;
        let rows = st.query_map(params![since.to_rfc3339()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (run_id, symbol, agent, output, reasoning, at) = row?;
            let (Some(agent), Ok(output)) =
                (AgentId::from_key(&agent), serde_json::from_str(&output))
            else {
                continue;
            };
            out.push(StoredOutput {
                run_id,
                symbol,
                agent,
                output,
                reasoning,
                created_at: ts(at),
            });
        }
        Ok(out)
    }

    pub fn upsert_signal(&self, s: &Signal) -> Result<()> {
        let state = serde_json::to_value(s.state)?;
        self.db().execute(
            "INSERT INTO signals (id, run_id, symbol, state, created_at, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET state = excluded.state, data = excluded.data",
            params![
                s.id,
                s.run_id,
                s.symbol,
                state.as_str().unwrap_or_default(),
                s.created_at.to_rfc3339(),
                serde_json::to_string(s)?
            ],
        )?;
        Ok(())
    }

    pub fn signals(&self, open_only: bool, limit: usize) -> Result<Vec<Signal>> {
        let db = self.db();
        let sql = if open_only {
            "SELECT data FROM signals WHERE state IN ('pending', 'triggered') ORDER BY created_at DESC LIMIT ?1"
        } else {
            "SELECT data FROM signals ORDER BY created_at DESC LIMIT ?1"
        };
        let mut st = db.prepare(sql)?;
        let rows = st.query_map(params![limit as i64], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            if let Ok(s) = serde_json::from_str(&row?) {
                out.push(s);
            }
        }
        Ok(out)
    }

    pub fn add_usage(&self, day: NaiveDate, agent: AgentId, u: &Usage) -> Result<()> {
        self.db().execute(
            "INSERT INTO usage (day, agent, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, web_searches, cost_usd)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(day, agent) DO UPDATE SET
               input_tokens = input_tokens + excluded.input_tokens,
               output_tokens = output_tokens + excluded.output_tokens,
               cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens,
               cache_write_tokens = cache_write_tokens + excluded.cache_write_tokens,
               web_searches = web_searches + excluded.web_searches,
               cost_usd = cost_usd + excluded.cost_usd",
            params![
                day.to_string(),
                agent.key(),
                u.input_tokens as i64,
                u.output_tokens as i64,
                u.cache_read_tokens as i64,
                u.cache_write_tokens as i64,
                u.web_searches as i64,
                u.cost_usd
            ],
        )?;
        Ok(())
    }

    pub fn usage_for_day(&self, day: NaiveDate) -> Result<Vec<(AgentId, Usage)>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT agent, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, web_searches, cost_usd
             FROM usage WHERE day = ?1",
        )?;
        let rows = st.query_map(params![day.to_string()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                Usage {
                    input_tokens: r.get::<_, i64>(1)? as u64,
                    output_tokens: r.get::<_, i64>(2)? as u64,
                    cache_read_tokens: r.get::<_, i64>(3)? as u64,
                    cache_write_tokens: r.get::<_, i64>(4)? as u64,
                    web_searches: r.get::<_, i64>(5)? as u64,
                    cost_usd: r.get(6)?,
                },
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (a, u) = row?;
            if let Some(a) = AgentId::from_key(&a) {
                out.push((a, u));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use merces_core::{Action, Decision, Side, SignalState, TradePlan};

    #[test]
    fn round_trips_runs_outputs_signals_usage() {
        let s = Store::in_memory().unwrap();
        let now = Utc::now();
        s.start_run("r1", "BTCUSDT", now).unwrap();
        let out = AgentOutput::Decision(Decision {
            action: Action::NoTrade,
            rationale: "flat".into(),
            risk_assessment: "high".into(),
            risk_score: 80,
            horizon: "day".into(),
            prediction: "range".into(),
        });
        s.add_output("r1", "BTCUSDT", AgentId::Decision, &out, "thinking", now)
            .unwrap();
        s.add_message("r1", AgentId::Research, AgentId::Chart, "bullish", now)
            .unwrap();
        s.finish_run(
            "r1",
            &RunOutcome::NoTrade {
                reason: "flat".into(),
            },
            now,
        )
        .unwrap();

        let got = s.outputs_since(now - chrono::Duration::hours(1)).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].output, out);
        let runs = s.recent_runs(5).unwrap();
        assert_eq!(
            runs[0].outcome,
            Some(RunOutcome::NoTrade {
                reason: "flat".into()
            })
        );
        assert!(s.last_run_at("BTCUSDT").unwrap().is_some());

        let plan = TradePlan {
            side: Side::Long,
            entry: 1.0,
            entry_window_start: now,
            entry_window_end: now,
            stop_loss: 0.9,
            take_profits: vec![1.2],
            risk_reward: 2.0,
            risk_pct: 0.5,
            confidence: 60,
            notes: String::new(),
        };
        let mut sig = Signal::new("s1".into(), "r1".into(), "BTCUSDT".into(), plan, 1.05, now);
        s.upsert_signal(&sig).unwrap();
        assert_eq!(s.signals(true, 10).unwrap().len(), 1);
        sig.state = SignalState::Expired;
        s.upsert_signal(&sig).unwrap();
        assert!(s.signals(true, 10).unwrap().is_empty());
        assert_eq!(s.signals(false, 10).unwrap()[0].state, SignalState::Expired);

        let day = now.date_naive();
        let u = Usage {
            input_tokens: 10,
            cost_usd: 0.5,
            ..Default::default()
        };
        s.add_usage(day, AgentId::Chart, &u).unwrap();
        s.add_usage(day, AgentId::Chart, &u).unwrap();
        let usage = s.usage_for_day(day).unwrap();
        assert_eq!(usage[0].1.input_tokens, 20);
        assert!((usage[0].1.cost_usd - 1.0).abs() < 1e-9);
    }
}

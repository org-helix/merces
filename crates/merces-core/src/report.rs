use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::RiskLimits;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Bias {
    Bullish,
    Bearish,
    #[default]
    Neutral,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalyst {
    pub title: String,
    pub impact: Bias,
    /// When it happens, free text ("today 14:30 UTC", "this week").
    pub when: String,
}

/// Agent 1 output: the state of the market.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketBrief {
    pub bias_day: Bias,
    pub bias_week: Bias,
    /// -100 (very bearish) … +100 (very bullish).
    pub sentiment: i32,
    pub summary: String,
    pub catalysts: Vec<Catalyst>,
    pub risks: Vec<String>,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    Confirms,
    Contradicts,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LevelKind {
    Support,
    Resistance,
    Pivot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyLevel {
    pub price: f64,
    pub kind: LevelKind,
    pub note: String,
}

/// Agent 2 output: what the charts say about agent 1's view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartView {
    pub stance: Stance,
    pub bias: Bias,
    /// Market structure per timeframe, e.g. "4h: HH/HL, above EMA200".
    pub structure: Vec<String>,
    pub levels: Vec<KeyLevel>,
    pub atr: f64,
    pub summary: String,
    /// 0-100.
    pub confidence: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Buy,
    Sell,
    NoTrade,
}

/// Agent 3 output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub action: Action,
    pub rationale: String,
    pub risk_assessment: String,
    /// 0 (no risk) … 100 (very risky).
    pub risk_score: u8,
    pub horizon: String,
    pub prediction: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Long,
    Short,
}

/// Agent 4 output: the trade itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradePlan {
    pub side: Side,
    pub entry: f64,
    pub entry_window_start: DateTime<Utc>,
    pub entry_window_end: DateTime<Utc>,
    pub stop_loss: f64,
    /// TP1 first.
    pub take_profits: Vec<f64>,
    pub risk_reward: f64,
    /// Share of the account to risk, in %.
    pub risk_pct: f64,
    /// 0-100.
    pub confidence: u8,
    pub notes: String,
}

impl TradePlan {
    /// Reward / risk to the first take-profit, recomputed from the prices.
    pub fn computed_rr(&self) -> f64 {
        let risk = (self.entry - self.stop_loss).abs();
        match self.take_profits.first() {
            Some(tp) if risk > 0.0 => (tp - self.entry).abs() / risk,
            _ => 0.0,
        }
    }

    /// Deterministic guardrails run after agent 4. Returns every problem found.
    pub fn validate(
        &self,
        last_price: f64,
        atr: f64,
        limits: &RiskLimits,
    ) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        let finite = [self.entry, self.stop_loss, self.risk_reward, self.risk_pct]
            .iter()
            .chain(self.take_profits.iter())
            .all(|v| v.is_finite());
        if !finite || self.entry <= 0.0 {
            errs.push("prices must be positive finite numbers".into());
            return Err(errs);
        }
        if self.take_profits.is_empty() {
            errs.push("at least one take-profit is required".into());
        }
        match self.side {
            Side::Long => {
                if self.stop_loss >= self.entry {
                    errs.push(format!(
                        "long: stop-loss {} must be below entry {}",
                        self.stop_loss, self.entry
                    ));
                }
                if self.take_profits.iter().any(|tp| *tp <= self.entry) {
                    errs.push("long: every take-profit must be above entry".into());
                }
            }
            Side::Short => {
                if self.stop_loss <= self.entry {
                    errs.push(format!(
                        "short: stop-loss {} must be above entry {}",
                        self.stop_loss, self.entry
                    ));
                }
                if self.take_profits.iter().any(|tp| *tp >= self.entry) {
                    errs.push("short: every take-profit must be below entry".into());
                }
            }
        }
        let rr = self.computed_rr();
        if rr + 1e-9 < limits.min_rr {
            errs.push(format!(
                "reward/risk to TP1 is {rr:.2}, minimum is {:.2}",
                limits.min_rr
            ));
        }
        if self.risk_pct > limits.max_risk_pct + 1e-9 {
            errs.push(format!(
                "risk {:.2}% exceeds the {:.2}% limit",
                self.risk_pct, limits.max_risk_pct
            ));
        }
        if atr > 0.0 && last_price > 0.0 {
            let dist = (self.entry - last_price).abs() / atr;
            if dist > limits.max_entry_atr {
                errs.push(format!(
                    "entry is {dist:.1} ATR from the current price {last_price}, max {:.1}",
                    limits.max_entry_atr
                ));
            }
        }
        if self.entry_window_end <= self.entry_window_start {
            errs.push("entry window end must be after its start".into());
        }
        if self.confidence > 100 {
            errs.push("confidence must be 0-100".into());
        }
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }
}

/// Anything an agent produces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum AgentOutput {
    Brief(MarketBrief),
    Chart(ChartView),
    Decision(Decision),
    Plan(TradePlan),
}

/// How a pipeline run ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunOutcome {
    Trade { signal_id: String },
    NoTrade { reason: String },
    Invalid { errors: Vec<String> },
    Failed { error: String },
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn plan(side: Side, entry: f64, sl: f64, tp: f64) -> TradePlan {
        let now = Utc::now();
        TradePlan {
            side,
            entry,
            entry_window_start: now,
            entry_window_end: now + Duration::hours(4),
            stop_loss: sl,
            take_profits: vec![tp],
            risk_reward: 2.0,
            risk_pct: 0.5,
            confidence: 70,
            notes: String::new(),
        }
    }

    #[test]
    fn valid_long_and_short_pass() {
        let l = RiskLimits::default();
        assert!(
            plan(Side::Long, 100.0, 98.0, 104.0)
                .validate(100.5, 2.0, &l)
                .is_ok()
        );
        assert!(
            plan(Side::Short, 100.0, 102.0, 96.0)
                .validate(99.5, 2.0, &l)
                .is_ok()
        );
    }

    #[test]
    fn stop_on_wrong_side_is_rejected() {
        let l = RiskLimits::default();
        let e = plan(Side::Long, 100.0, 101.0, 104.0)
            .validate(100.0, 2.0, &l)
            .unwrap_err();
        assert!(e.iter().any(|m| m.contains("stop-loss")));
        let e = plan(Side::Short, 100.0, 99.0, 96.0)
            .validate(100.0, 2.0, &l)
            .unwrap_err();
        assert!(e.iter().any(|m| m.contains("stop-loss")));
    }

    #[test]
    fn poor_reward_risk_is_rejected() {
        let l = RiskLimits::default();
        let e = plan(Side::Long, 100.0, 98.0, 101.0)
            .validate(100.0, 2.0, &l)
            .unwrap_err();
        assert!(e.iter().any(|m| m.contains("reward/risk")));
    }

    #[test]
    fn entry_far_from_price_is_rejected() {
        let l = RiskLimits::default();
        let e = plan(Side::Long, 100.0, 98.0, 104.0)
            .validate(120.0, 2.0, &l)
            .unwrap_err();
        assert!(e.iter().any(|m| m.contains("ATR")));
    }

    #[test]
    fn output_round_trips_as_json() {
        let o = AgentOutput::Plan(plan(Side::Long, 1.0, 0.9, 1.2));
        let s = serde_json::to_string(&o).unwrap();
        assert_eq!(serde_json::from_str::<AgentOutput>(&s).unwrap(), o);
    }
}

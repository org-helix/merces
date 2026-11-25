use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Side, TradePlan};

/// Paper-tracking state of a signal. No order is ever sent to a broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SignalState {
    /// Waiting for the price to reach the entry inside the window.
    #[default]
    Pending,
    /// Entry reached: the simulated position is open.
    Triggered,
    TakeProfit,
    StopLoss,
    /// The window closed before the entry was reached.
    Expired,
}

impl SignalState {
    pub fn is_closed(self) -> bool {
        matches!(
            self,
            SignalState::TakeProfit | SignalState::StopLoss | SignalState::Expired
        )
    }
}

/// A trade plan followed on paper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub id: String,
    pub run_id: String,
    pub symbol: String,
    pub plan: TradePlan,
    pub state: SignalState,
    pub created_at: DateTime<Utc>,
    pub triggered_at: Option<DateTime<Utc>>,
    pub closed_at: Option<DateTime<Utc>>,
    pub exit_price: Option<f64>,
    /// Result in R multiples (1R = the distance entry→stop).
    pub pnl_r: Option<f64>,
    /// Price when the plan was issued: tells a limit entry from a stop (breakout) entry.
    pub ref_price: f64,
    /// Guardrail warnings kept with the signal (e.g. confidence under the threshold).
    pub flagged: Vec<String>,
}

/// Positions still open after this long are closed at market on paper.
const MAX_HOLD_HOURS: i64 = 72;

impl Signal {
    pub fn new(
        id: String,
        run_id: String,
        symbol: String,
        plan: TradePlan,
        ref_price: f64,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            run_id,
            symbol,
            plan,
            state: SignalState::Pending,
            created_at: now,
            triggered_at: None,
            closed_at: None,
            exit_price: None,
            pnl_r: None,
            ref_price,
            flagged: Vec::new(),
        }
    }

    fn r_multiple(&self, exit: f64) -> f64 {
        let p = &self.plan;
        let risk = (p.entry - p.stop_loss).abs();
        if risk <= 0.0 {
            return 0.0;
        }
        match p.side {
            Side::Long => (exit - p.entry) / risk,
            Side::Short => (p.entry - exit) / risk,
        }
    }

    fn close(&mut self, state: SignalState, exit: Option<f64>, now: DateTime<Utc>) {
        self.state = state;
        self.closed_at = Some(now);
        self.exit_price = exit;
        self.pnl_r = exit.map(|x| self.r_multiple(x));
    }

    /// Feed one price observation. Returns true when the state changed.
    ///
    /// Without intra-bar data the check is on the observed price only; when both
    /// SL and TP would be crossed in one step the stop wins (pessimistic).
    pub fn update(&mut self, price: f64, now: DateTime<Utc>) -> bool {
        if self.state.is_closed() || !price.is_finite() {
            return false;
        }
        let p = self.plan.clone();
        let before = self.state;
        if self.state == SignalState::Pending {
            if now > p.entry_window_end {
                self.close(SignalState::Expired, None, now);
                return true;
            }
            if now < p.entry_window_start {
                return false;
            }
            // Entry above the issue price = stop/breakout entry, below = limit entry.
            let reached = if self.ref_price > 0.0 && self.ref_price < p.entry {
                price >= p.entry
            } else if self.ref_price > p.entry {
                price <= p.entry
            } else {
                match p.side {
                    Side::Long => price <= p.entry,
                    Side::Short => price >= p.entry,
                }
            };
            if reached {
                self.state = SignalState::Triggered;
                self.triggered_at = Some(now);
            }
        }
        if self.state == SignalState::Triggered {
            let tp = p.take_profits.first().copied();
            let (hit_sl, hit_tp) = match p.side {
                Side::Long => (price <= p.stop_loss, tp.is_some_and(|t| price >= t)),
                Side::Short => (price >= p.stop_loss, tp.is_some_and(|t| price <= t)),
            };
            if hit_sl {
                self.close(SignalState::StopLoss, Some(p.stop_loss), now);
            } else if hit_tp {
                self.close(SignalState::TakeProfit, tp, now);
            } else if self
                .triggered_at
                .is_some_and(|t| (now - t).num_hours() >= MAX_HOLD_HOURS)
            {
                // Time stop: closed at the observed price. Counted as SL/TP by sign.
                let state = if self.r_multiple(price) >= 0.0 {
                    SignalState::TakeProfit
                } else {
                    SignalState::StopLoss
                };
                self.close(state, Some(price), now);
            }
        }
        self.state != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn signal(side: Side) -> (Signal, DateTime<Utc>) {
        let now = Utc::now();
        let (entry, sl, tp) = match side {
            Side::Long => (100.0, 98.0, 104.0),
            Side::Short => (100.0, 102.0, 96.0),
        };
        let plan = TradePlan {
            side,
            entry,
            entry_window_start: now,
            entry_window_end: now + Duration::hours(2),
            stop_loss: sl,
            take_profits: vec![tp],
            risk_reward: 2.0,
            risk_pct: 0.5,
            confidence: 60,
            notes: String::new(),
        };
        let ref_price = match side {
            Side::Long => 101.0,
            Side::Short => 99.0,
        };
        (
            Signal::new("s".into(), "r".into(), "X".into(), plan, ref_price, now),
            now,
        )
    }

    #[test]
    fn long_triggers_then_hits_tp() {
        let (mut s, t) = signal(Side::Long);
        assert!(!s.update(101.0, t));
        assert!(s.update(99.9, t + Duration::minutes(5)));
        assert_eq!(s.state, SignalState::Triggered);
        assert!(s.update(104.2, t + Duration::minutes(30)));
        assert_eq!(s.state, SignalState::TakeProfit);
        assert!((s.pnl_r.unwrap() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn short_hits_stop() {
        let (mut s, t) = signal(Side::Short);
        s.update(100.1, t);
        assert_eq!(s.state, SignalState::Triggered);
        s.update(102.5, t + Duration::minutes(1));
        assert_eq!(s.state, SignalState::StopLoss);
        assert!((s.pnl_r.unwrap() + 1.0).abs() < 1e-9);
    }

    #[test]
    fn breakout_long_waits_for_price_above_entry() {
        let (mut s, t) = signal(Side::Long);
        s.ref_price = 99.0;
        assert!(!s.update(99.5, t));
        assert!(s.update(100.2, t + Duration::minutes(1)));
        assert_eq!(s.state, SignalState::Triggered);
    }

    #[test]
    fn untouched_entry_expires() {
        let (mut s, t) = signal(Side::Long);
        s.update(103.0, t + Duration::hours(3));
        assert_eq!(s.state, SignalState::Expired);
        assert_eq!(s.pnl_r, None);
        assert!(!s.update(90.0, t + Duration::hours(4)));
    }
}

use chrono::NaiveTime;
use serde::{Deserialize, Serialize};

/// Where a symbol's prices come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// Crypto, public API, no key.
    #[default]
    Binance,
    /// Forex, indices, metals (key required).
    TwelveData,
    /// US stocks (key id + secret required).
    Alpaca,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::Binance, Provider::TwelveData, Provider::Alpaca];
    pub fn label(self) -> &'static str {
        match self {
            Provider::Binance => "Binance",
            Provider::TwelveData => "Twelve Data",
            Provider::Alpaca => "Alpaca",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AssetClass {
    #[default]
    Crypto,
    Forex,
    Index,
    Commodity,
    Stock,
}

impl AssetClass {
    pub const ALL: [AssetClass; 5] = [
        AssetClass::Crypto,
        AssetClass::Forex,
        AssetClass::Index,
        AssetClass::Commodity,
        AssetClass::Stock,
    ];
    pub fn default_provider(self) -> Provider {
        match self {
            AssetClass::Crypto => Provider::Binance,
            AssetClass::Stock => Provider::Alpaca,
            _ => Provider::TwelveData,
        }
    }
}

/// Risk limits the decision and execution agents must respect (checked again in Rust).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RiskLimits {
    /// Minimum reward / risk ratio to the first take-profit.
    pub min_rr: f64,
    /// Max share of the account risked on one trade, in %.
    pub max_risk_pct: f64,
    /// Below this confidence (0-100) the plan is shown but flagged.
    pub min_confidence: u8,
    /// How far the entry may sit from the current price, in ATR multiples.
    pub max_entry_atr: f64,
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            min_rr: 1.5,
            max_risk_pct: 1.0,
            min_confidence: 55,
            max_entry_atr: 2.0,
        }
    }
}

/// A traded symbol, configured by the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SymbolConfig {
    /// Provider symbol: `BTCUSDT`, `EUR/USD`, `XAU/USD`, `SPX`, `AAPL`…
    pub symbol: String,
    pub name: String,
    pub class: AssetClass,
    pub provider: Provider,
    /// Daily automatic run (UTC). `None` = manual only.
    pub schedule_utc: Option<NaiveTime>,
    pub enabled: bool,
    /// Overrides the global limits when set.
    pub risk: Option<RiskLimits>,
}

impl Default for SymbolConfig {
    fn default() -> Self {
        Self {
            symbol: "BTCUSDT".into(),
            name: "Bitcoin".into(),
            class: AssetClass::Crypto,
            provider: Provider::Binance,
            schedule_utc: NaiveTime::from_hms_opt(6, 30, 0),
            enabled: true,
            risk: None,
        }
    }
}

/// A search result when adding a symbol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub symbol: String,
    pub name: String,
    pub class: AssetClass,
    pub provider: Provider,
    pub exchange: String,
}

/// OHLCV candle. `t` is the open time in ms since the epoch (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Candle {
    pub t: i64,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    pub v: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Timeframe {
    M15,
    H1,
    H4,
    D1,
}

impl Timeframe {
    pub const ANALYSIS: [Timeframe; 4] =
        [Timeframe::D1, Timeframe::H4, Timeframe::H1, Timeframe::M15];
    pub fn label(self) -> &'static str {
        match self {
            Timeframe::M15 => "15m",
            Timeframe::H1 => "1h",
            Timeframe::H4 => "4h",
            Timeframe::D1 => "1D",
        }
    }
    pub fn minutes(self) -> i64 {
        match self {
            Timeframe::M15 => 15,
            Timeframe::H1 => 60,
            Timeframe::H4 => 240,
            Timeframe::D1 => 1440,
        }
    }
}

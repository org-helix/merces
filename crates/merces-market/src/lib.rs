//! Market data. One [`Market`] value serves every provider; each call is routed by
//! the symbol's [`Provider`]. Only read-only data endpoints are used: Merces never
//! places orders.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use merces_core::{AssetClass, Candle, Provider, SymbolConfig, SymbolInfo, Timeframe};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::OnceCell;

#[derive(Debug, thiserror::Error)]
pub enum MarketError {
    #[error("network: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{provider}: {message}")]
    Api {
        provider: &'static str,
        message: String,
    },
    #[error("{0} API key missing (Settings → Data)")]
    MissingKey(&'static str),
    #[error("unexpected data: {0}")]
    Parse(String),
}

pub type Result<T> = std::result::Result<T, MarketError>;

/// API keys, read from the OS keyring by the app.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarketKeys {
    pub twelve_data: Option<String>,
    pub alpaca_key_id: Option<String>,
    pub alpaca_secret: Option<String>,
    pub finnhub: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewsItem {
    pub headline: String,
    pub summary: String,
    pub source: String,
    pub url: String,
    pub datetime: DateTime<Utc>,
}

/// Read-only data access used by the agents. Implemented by [`Market`] and by
/// test doubles.
pub trait MarketSource: Send + Sync + 'static {
    fn candles(
        &self,
        s: &SymbolConfig,
        tf: Timeframe,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<Candle>>> + Send;
    fn last_price(&self, s: &SymbolConfig) -> impl Future<Output = Result<f64>> + Send;
    fn news(
        &self,
        s: &SymbolConfig,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<NewsItem>>> + Send;
}

#[derive(Clone)]
pub struct Market {
    http: reqwest::Client,
    keys: Arc<std::sync::RwLock<MarketKeys>>,
    binance_symbols: Arc<OnceCell<Vec<SymbolInfo>>>,
}

impl Default for Market {
    fn default() -> Self {
        Self::new(MarketKeys::default())
    }
}

impl Market {
    pub fn new(keys: MarketKeys) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("merces/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .build()
            .expect("HTTP client");
        Self {
            http,
            keys: Arc::new(std::sync::RwLock::new(keys)),
            binance_symbols: Arc::new(OnceCell::new()),
        }
    }

    pub fn set_keys(&self, keys: MarketKeys) {
        *self.keys.write().expect("keys lock") = keys;
    }

    fn keys(&self) -> MarketKeys {
        self.keys.read().expect("keys lock").clone()
    }

    async fn get_json(
        &self,
        req: reqwest::RequestBuilder,
        provider: &'static str,
    ) -> Result<Value> {
        let resp = req.send().await?;
        let status = resp.status();
        let body: Value = resp.json().await.map_err(MarketError::Http)?;
        if !status.is_success() {
            let message = body
                .get("msg")
                .or_else(|| body.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(MarketError::Api { provider, message });
        }
        // Twelve Data reports errors with HTTP 200.
        if body.get("status").and_then(Value::as_str) == Some("error") {
            let message = body["message"].as_str().unwrap_or("error").to_owned();
            return Err(MarketError::Api { provider, message });
        }
        Ok(body)
    }

    /// Search symbols across the providers that are usable right now.
    pub async fn search(&self, query: &str) -> Result<Vec<SymbolInfo>> {
        let q = query.trim().to_uppercase();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        if let Ok(list) = self.binance_list().await {
            out.extend(
                list.iter()
                    .filter(|s| s.symbol.starts_with(&q))
                    .take(12)
                    .cloned(),
            );
        }
        let keys = self.keys();
        if let Some(k) = keys.twelve_data.as_deref() {
            let body = self
                .get_json(
                    self.http
                        .get("https://api.twelvedata.com/symbol_search")
                        .query(&[("symbol", q.as_str()), ("outputsize", "20"), ("apikey", k)]),
                    "Twelve Data",
                )
                .await;
            if let Ok(body) = body {
                for d in body["data"].as_array().into_iter().flatten().take(20) {
                    let ty = d["instrument_type"].as_str().unwrap_or_default();
                    let class = match ty {
                        t if t.contains("Physical Currency") => AssetClass::Forex,
                        t if t.contains("Index") => AssetClass::Index,
                        t if t.contains("Digital Currency") => AssetClass::Crypto,
                        t if t.contains("Commodit") => AssetClass::Commodity,
                        _ => AssetClass::Stock,
                    };
                    out.push(SymbolInfo {
                        symbol: d["symbol"].as_str().unwrap_or_default().to_owned(),
                        name: d["instrument_name"].as_str().unwrap_or_default().to_owned(),
                        class,
                        provider: Provider::TwelveData,
                        exchange: d["exchange"].as_str().unwrap_or_default().to_owned(),
                    });
                }
            }
        }
        if keys.alpaca_key_id.is_some()
            && q.chars().all(|c| c.is_ascii_alphabetic() || c == '.')
            && q.len() <= 6
        {
            out.push(SymbolInfo {
                symbol: q.clone(),
                name: format!("{q} (US)"),
                class: AssetClass::Stock,
                provider: Provider::Alpaca,
                exchange: "US".into(),
            });
        }
        Ok(out)
    }

    async fn binance_list(&self) -> Result<&Vec<SymbolInfo>> {
        self.binance_symbols
            .get_or_try_init(|| async {
                let body = self
                    .get_json(
                        self.http
                            .get("https://api.binance.com/api/v3/exchangeInfo")
                            .query(&[("permissions", "SPOT"), ("symbolStatus", "TRADING")]),
                        "Binance",
                    )
                    .await?;
                Ok(body["symbols"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|s| SymbolInfo {
                        symbol: s["symbol"].as_str().unwrap_or_default().to_owned(),
                        name: format!(
                            "{} / {}",
                            s["baseAsset"].as_str().unwrap_or_default(),
                            s["quoteAsset"].as_str().unwrap_or_default()
                        ),
                        class: AssetClass::Crypto,
                        provider: Provider::Binance,
                        exchange: "Binance".into(),
                    })
                    .collect())
            })
            .await
    }

    // ── Binance ────────────────────────────────────────────────────────────
    async fn binance_candles(
        &self,
        symbol: &str,
        tf: Timeframe,
        limit: usize,
    ) -> Result<Vec<Candle>> {
        let interval = match tf {
            Timeframe::M15 => "15m",
            Timeframe::H1 => "1h",
            Timeframe::H4 => "4h",
            Timeframe::D1 => "1d",
        };
        let body = self
            .get_json(
                self.http
                    .get("https://api.binance.com/api/v3/klines")
                    .query(&[
                        ("symbol", symbol),
                        ("interval", interval),
                        ("limit", &limit.min(1000).to_string()),
                    ]),
                "Binance",
            )
            .await?;
        parse_binance_klines(&body)
    }

    async fn binance_price(&self, symbol: &str) -> Result<f64> {
        let body = self
            .get_json(
                self.http
                    .get("https://api.binance.com/api/v3/ticker/price")
                    .query(&[("symbol", symbol)]),
                "Binance",
            )
            .await?;
        num(&body["price"]).ok_or_else(|| MarketError::Parse("Binance price".into()))
    }

    // ── Twelve Data ────────────────────────────────────────────────────────
    async fn twelve_candles(
        &self,
        symbol: &str,
        tf: Timeframe,
        limit: usize,
    ) -> Result<Vec<Candle>> {
        let key = self
            .keys()
            .twelve_data
            .ok_or(MarketError::MissingKey("Twelve Data"))?;
        let interval = match tf {
            Timeframe::M15 => "15min",
            Timeframe::H1 => "1h",
            Timeframe::H4 => "4h",
            Timeframe::D1 => "1day",
        };
        let body = self
            .get_json(
                self.http
                    .get("https://api.twelvedata.com/time_series")
                    .query(&[
                        ("symbol", symbol),
                        ("interval", interval),
                        ("outputsize", &limit.min(5000).to_string()),
                        ("timezone", "UTC"),
                        ("order", "ASC"),
                        ("apikey", &key),
                    ]),
                "Twelve Data",
            )
            .await?;
        parse_twelve_series(&body)
    }

    async fn twelve_price(&self, symbol: &str) -> Result<f64> {
        let key = self
            .keys()
            .twelve_data
            .ok_or(MarketError::MissingKey("Twelve Data"))?;
        let body = self
            .get_json(
                self.http
                    .get("https://api.twelvedata.com/price")
                    .query(&[("symbol", symbol), ("apikey", &key)]),
                "Twelve Data",
            )
            .await?;
        num(&body["price"]).ok_or_else(|| MarketError::Parse("Twelve Data price".into()))
    }

    // ── Alpaca ─────────────────────────────────────────────────────────────
    fn alpaca(&self, url: String) -> Result<reqwest::RequestBuilder> {
        let k = self.keys();
        let id = k.alpaca_key_id.ok_or(MarketError::MissingKey("Alpaca"))?;
        let secret = k.alpaca_secret.ok_or(MarketError::MissingKey("Alpaca"))?;
        Ok(self
            .http
            .get(url)
            .header("APCA-API-KEY-ID", id)
            .header("APCA-API-SECRET-KEY", secret))
    }

    async fn alpaca_candles(
        &self,
        symbol: &str,
        tf: Timeframe,
        limit: usize,
    ) -> Result<Vec<Candle>> {
        let timeframe = match tf {
            Timeframe::M15 => "15Min",
            Timeframe::H1 => "1Hour",
            Timeframe::H4 => "4Hour",
            Timeframe::D1 => "1Day",
        };
        // Markets are open ~1/4 of the time: look far enough back, newest first.
        let span = chrono::Duration::minutes(tf.minutes() * limit as i64 * 5);
        let start = (Utc::now() - span).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let req = self
            .alpaca(format!(
                "https://data.alpaca.markets/v2/stocks/{symbol}/bars"
            ))?
            .query(&[
                ("timeframe", timeframe),
                ("limit", &limit.min(10_000).to_string()),
                ("start", &start),
                ("sort", "desc"),
                ("feed", "iex"),
                ("adjustment", "split"),
            ]);
        let body = self.get_json(req, "Alpaca").await?;
        let mut out = Vec::new();
        for b in body["bars"].as_array().into_iter().flatten() {
            let t = b["t"]
                .as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.timestamp_millis())
                .ok_or_else(|| MarketError::Parse("Alpaca bar time".into()))?;
            out.push(Candle {
                t,
                o: num(&b["o"]).unwrap_or(f64::NAN),
                h: num(&b["h"]).unwrap_or(f64::NAN),
                l: num(&b["l"]).unwrap_or(f64::NAN),
                c: num(&b["c"]).unwrap_or(f64::NAN),
                v: num(&b["v"]).unwrap_or(0.0),
            });
        }
        out.reverse();
        Ok(out)
    }

    async fn alpaca_price(&self, symbol: &str) -> Result<f64> {
        let req = self
            .alpaca(format!(
                "https://data.alpaca.markets/v2/stocks/{symbol}/trades/latest"
            ))?
            .query(&[("feed", "iex")]);
        let body = self.get_json(req, "Alpaca").await?;
        num(&body["trade"]["p"]).ok_or_else(|| MarketError::Parse("Alpaca price".into()))
    }

    // ── Finnhub news ───────────────────────────────────────────────────────
    async fn finnhub_news(&self, s: &SymbolConfig, limit: usize) -> Result<Vec<NewsItem>> {
        let Some(token) = self.keys().finnhub else {
            return Ok(Vec::new());
        };
        let req = if s.class == AssetClass::Stock {
            let to = Utc::now().date_naive();
            let from = to - chrono::Duration::days(3);
            self.http
                .get("https://finnhub.io/api/v1/company-news")
                .query(&[
                    ("symbol", s.symbol.as_str()),
                    ("from", &from.to_string()),
                    ("to", &to.to_string()),
                    ("token", &token),
                ])
        } else {
            let category = match s.class {
                AssetClass::Crypto => "crypto",
                AssetClass::Forex => "forex",
                _ => "general",
            };
            self.http
                .get("https://finnhub.io/api/v1/news")
                .query(&[("category", category), ("token", token.as_str())])
        };
        let body = self.get_json(req, "Finnhub").await?;
        Ok(body
            .as_array()
            .into_iter()
            .flatten()
            .take(limit)
            .map(|n| NewsItem {
                headline: n["headline"].as_str().unwrap_or_default().to_owned(),
                summary: n["summary"]
                    .as_str()
                    .unwrap_or_default()
                    .chars()
                    .take(400)
                    .collect(),
                source: n["source"].as_str().unwrap_or_default().to_owned(),
                url: n["url"].as_str().unwrap_or_default().to_owned(),
                datetime: n["datetime"]
                    .as_i64()
                    .and_then(|t| DateTime::from_timestamp(t, 0))
                    .unwrap_or_else(Utc::now),
            })
            .collect())
    }
}

impl MarketSource for Market {
    async fn candles(&self, s: &SymbolConfig, tf: Timeframe, limit: usize) -> Result<Vec<Candle>> {
        let mut c = match s.provider {
            Provider::Binance => self.binance_candles(&s.symbol, tf, limit).await?,
            Provider::TwelveData => self.twelve_candles(&s.symbol, tf, limit).await?,
            Provider::Alpaca => self.alpaca_candles(&s.symbol, tf, limit).await?,
        };
        c.retain(|k| k.c.is_finite() && k.h.is_finite() && k.l.is_finite());
        Ok(c)
    }

    async fn last_price(&self, s: &SymbolConfig) -> Result<f64> {
        match s.provider {
            Provider::Binance => self.binance_price(&s.symbol).await,
            Provider::TwelveData => self.twelve_price(&s.symbol).await,
            Provider::Alpaca => self.alpaca_price(&s.symbol).await,
        }
    }

    async fn news(&self, s: &SymbolConfig, limit: usize) -> Result<Vec<NewsItem>> {
        self.finnhub_news(s, limit).await
    }
}

/// Numbers arrive as JSON numbers or as strings depending on the provider.
fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn parse_binance_klines(body: &Value) -> Result<Vec<Candle>> {
    body.as_array()
        .ok_or_else(|| MarketError::Parse("Binance klines".into()))?
        .iter()
        .map(|k| {
            let f = |i: usize| {
                num(&k[i]).ok_or_else(|| MarketError::Parse(format!("Binance kline field {i}")))
            };
            Ok(Candle {
                t: k[0]
                    .as_i64()
                    .ok_or_else(|| MarketError::Parse("Binance kline time".into()))?,
                o: f(1)?,
                h: f(2)?,
                l: f(3)?,
                c: f(4)?,
                v: f(5)?,
            })
        })
        .collect()
}

fn parse_twelve_series(body: &Value) -> Result<Vec<Candle>> {
    let values = body["values"]
        .as_array()
        .ok_or_else(|| MarketError::Parse("Twelve Data values".into()))?;
    let mut out = Vec::with_capacity(values.len());
    for v in values {
        let dt = v["datetime"].as_str().unwrap_or_default();
        let t = chrono::NaiveDateTime::parse_from_str(dt, "%Y-%m-%d %H:%M:%S")
            .or_else(|_| {
                chrono::NaiveDate::parse_from_str(dt, "%Y-%m-%d")
                    .map(|d| d.and_hms_opt(0, 0, 0).unwrap_or_default())
            })
            .map_err(|_| MarketError::Parse(format!("Twelve Data datetime {dt}")))?
            .and_utc()
            .timestamp_millis();
        out.push(Candle {
            t,
            o: num(&v["open"]).unwrap_or(f64::NAN),
            h: num(&v["high"]).unwrap_or(f64::NAN),
            l: num(&v["low"]).unwrap_or(f64::NAN),
            c: num(&v["close"]).unwrap_or(f64::NAN),
            v: num(&v["volume"]).unwrap_or(0.0),
        });
    }
    out.sort_by_key(|c| c.t);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_binance_klines() {
        let body = json!([[
            1700000000000i64,
            "1.0",
            "2.0",
            "0.5",
            "1.5",
            "100",
            0,
            "0",
            1,
            "0",
            "0",
            "0"
        ]]);
        let c = parse_binance_klines(&body).unwrap();
        assert_eq!(c[0].t, 1_700_000_000_000);
        assert_eq!(
            (c[0].o, c[0].h, c[0].l, c[0].c, c[0].v),
            (1.0, 2.0, 0.5, 1.5, 100.0)
        );
    }

    #[test]
    fn parses_twelve_series_and_sorts() {
        let body = json!({"values": [
            {"datetime": "2026-01-02", "open": "2", "high": "3", "low": "1", "close": "2.5"},
            {"datetime": "2026-01-01 10:00:00", "open": "1", "high": "2", "low": "0.5", "close": "1.5", "volume": "9"}
        ], "status": "ok"});
        let c = parse_twelve_series(&body).unwrap();
        assert!(c[0].t < c[1].t);
        assert_eq!(c[0].v, 9.0);
        assert_eq!(c[1].v, 0.0);
    }

    #[test]
    fn numbers_from_strings_or_numbers() {
        assert_eq!(num(&json!("1.25")), Some(1.25));
        assert_eq!(num(&json!(2)), Some(2.0));
        assert_eq!(num(&json!(null)), None);
    }
}

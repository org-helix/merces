//! Offline demo mode: a simulated Claude and a simulated market, so the app can be
//! explored (and tested) without API keys or network. Also the runtime switch
//! between real and demo backends.

use std::time::Duration;

use chrono::{Duration as CDuration, Utc};
use merces_core::{Candle, SymbolConfig, Timeframe, Usage};
use merces_llm::{Claude, LlmBackend, Request, Response, Result as LlmResult, StreamEvent};
use merces_market::{Market, MarketSource, NewsItem, Result as MarketResult};
use serde_json::{Value, json};

/// Small deterministic PRNG (xorshift64*).
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

fn base_price(symbol: &str) -> f64 {
    let s = symbol.to_uppercase();
    match () {
        _ if s.starts_with("BTC") => 64_000.0,
        _ if s.starts_with("ETH") => 3_200.0,
        _ if s.starts_with("SOL") => 150.0,
        _ if s.contains("XAU") => 2_400.0,
        _ if s.contains("EUR/USD") || s == "EURUSD" => 1.08,
        _ if s.contains("JPY") => 150.0,
        _ if s.contains("SPX") || s.contains("US500") => 5_500.0,
        _ if s.contains("NDX") || s.contains("NAS") => 19_000.0,
        _ => 20.0 + (hash(&s) % 400) as f64,
    }
}

/// Random-walk market, stable per symbol and continuous in time.
#[derive(Clone, Default)]
pub struct DemoMarket;

impl DemoMarket {
    fn series(symbol: &str, tf: Timeframe, limit: usize) -> Vec<Candle> {
        let step_ms = tf.minutes() * 60_000;
        let now = Utc::now().timestamp_millis();
        let last_open = now - now.rem_euclid(step_ms);
        let first = last_open - step_ms * (limit as i64 - 1);
        // Walk from a fixed origin so overlapping timeframes agree roughly.
        let mut rng = Rng::new(hash(symbol) ^ tf.minutes() as u64);
        let vol = 0.004 * (tf.minutes() as f64 / 60.0).sqrt();
        let mut price = base_price(symbol);
        let mut out = Vec::with_capacity(limit);
        let drift = ((hash(symbol) % 7) as f64 - 3.0) * 0.0002;
        for i in 0..limit as i64 {
            let o = price;
            let wave = (i as f64 / 17.0).sin() * vol * 0.6;
            let r = (rng.f64() - 0.5) * 2.0 * vol + drift + wave * 0.2;
            let c = (o * (1.0 + r)).max(o * 0.5);
            let h = o.max(c) * (1.0 + rng.f64() * vol * 0.5);
            let l = o.min(c) * (1.0 - rng.f64() * vol * 0.5);
            out.push(Candle {
                t: first + i * step_ms,
                o,
                h,
                l,
                c,
                v: 100.0 + rng.f64() * 900.0,
            });
            price = c;
        }
        out
    }

    fn live_price(symbol: &str) -> f64 {
        let c = Self::series(symbol, Timeframe::M15, 120);
        let last = c.last().map_or(base_price(symbol), |k| k.c);
        // Small oscillation within the bar so the paper tracker sees movement.
        let t = Utc::now().timestamp() as f64;
        last * (1.0 + (t / 40.0 + hash(symbol) as f64).sin() * 0.0015)
    }
}

impl MarketSource for DemoMarket {
    async fn candles(
        &self,
        s: &SymbolConfig,
        tf: Timeframe,
        limit: usize,
    ) -> MarketResult<Vec<Candle>> {
        let mut c = Self::series(&s.symbol, tf, limit);
        // Align the last close with the live price.
        let p = Self::live_price(&s.symbol);
        if let Some(last) = c.last_mut() {
            let k = p / last.c;
            last.c = p;
            last.h = last.h.max(p);
            last.l = last.l.min(p);
            last.o *= k.clamp(0.98, 1.02);
        }
        Ok(c)
    }

    async fn last_price(&self, s: &SymbolConfig) -> MarketResult<f64> {
        Ok(Self::live_price(&s.symbol))
    }

    async fn news(&self, _s: &SymbolConfig, _limit: usize) -> MarketResult<Vec<NewsItem>> {
        Ok(vec![NewsItem {
            headline: "Demo: central bank speakers expected this afternoon".into(),
            summary: String::new(),
            source: "Merces demo".into(),
            url: String::new(),
            datetime: Utc::now(),
        }])
    }
}

/// Simulated Claude: streams plausible reasoning and returns valid structured outputs.
#[derive(Clone)]
pub struct DemoLlm {
    /// Delay between streamed words.
    pub pace: Duration,
}

impl Default for DemoLlm {
    fn default() -> Self {
        Self {
            pace: Duration::from_millis(35),
        }
    }
}

fn find_num(text: &str, label: &str) -> Option<f64> {
    let i = text.find(label)? + label.len();
    let s: String = text[i..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    s.trim_end_matches('.').parse().ok()
}

impl DemoLlm {
    fn answer(&self, agent: &str, user: &str, rng: &mut Rng) -> (String, Value) {
        let price = find_num(user, "Last price:").unwrap_or(100.0);
        let atr = find_num(user, "Reference ATR (4h, 14):").unwrap_or(price * 0.01);
        let up = rng.f64() > 0.45;
        let bias = if up { "bullish" } else { "bearish" };
        match agent {
            "research" => (
                "Je passe en revue le contexte macro, le calendrier économique du jour et les flux récents. \
                 Les publications de la semaine pèsent sur la volatilité ; le sentiment des dernières séances reste \
                 orienté dans le même sens que la tendance de fond."
                    .into(),
                json!({
                    "bias_day": bias, "bias_week": if rng.f64() > 0.3 { bias } else { "neutral" },
                    "sentiment": ((rng.f64() * 70.0) as i32 + 10) * if up { 1 } else { -1 },
                    "summary": "Mode démo : contexte simulé. Le marché évolue dans une tendance modérée, avec des catalyseurs macro attendus en séance américaine. Les flux restent porteurs mais un événement de calendrier pourrait accroître la volatilité.",
                    "catalysts": [
                        {"title": "Discours de banquiers centraux", "impact": "neutral", "when": "aujourd'hui 14:00 UTC"},
                        {"title": "Flux acheteurs institutionnels", "impact": bias, "when": "cette semaine"}
                    ],
                    "risks": ["Surprise sur l'inflation", "Liquidité réduite en fin de séance"],
                    "sources": ["https://example.org/demo"]
                }),
            ),
            "chart" => {
                let s1 = price - atr * 1.2;
                let r1 = price + atr * 1.4;
                (
                    "Le 1D et le 4h montrent une structure cohérente ; le RSI reste hors des extrêmes. Sur 1h, \
                     le prix teste la moyenne mobile 20 et respecte le dernier creux."
                        .into(),
                    json!({
                        "stance": if rng.f64() > 0.25 { "confirms" } else { "mixed" },
                        "bias": bias,
                        "structure": ["1D : tendance intacte au-dessus de l'EMA200", "4h : plus hauts/plus bas croissants", "1h : consolidation sur l'EMA20"],
                        "levels": [
                            {"price": s1, "kind": "support", "note": "dernier creux 4h"},
                            {"price": r1, "kind": "resistance", "note": "sommet récent"}
                        ],
                        "atr": atr,
                        "summary": "Mode démo : lecture simulée des graphiques, cohérente avec le biais de la recherche.",
                        "confidence": 55 + (rng.f64() * 30.0) as i32
                    }),
                )
            }
            "decision" => {
                let action = if rng.f64() < 0.25 { "no_trade" } else if up { "buy" } else { "sell" };
                (
                    "Les deux analyses vont globalement dans le même sens ; je pèse le risque d'événement \
                     contre le potentiel jusqu'à la prochaine résistance."
                        .into(),
                    json!({
                        "action": action,
                        "rationale": if action == "no_trade" { "Mode démo : signaux contradictoires et événement macro proche, on reste à l'écart aujourd'hui." } else { "Mode démo : recherche et graphiques alignés, niveau d'invalidation clair." },
                        "risk_assessment": "Volatilité autour des annonces macro.",
                        "risk_score": 30 + (rng.f64() * 40.0) as i32,
                        "horizon": "intraday à 2 jours",
                        "prediction": "Poursuite du mouvement vers la prochaine zone de liquidité."
                    }),
                )
            }
            "execution" => {
                let long = user.contains("build the long trade plan");
                let entry = if long { price - atr * 0.3 } else { price + atr * 0.3 };
                let sl = if long { entry - atr * 1.0 } else { entry + atr * 1.0 };
                let tp1 = if long { entry + atr * 2.0 } else { entry - atr * 2.0 };
                let tp2 = if long { entry + atr * 3.2 } else { entry - atr * 3.2 };
                let now = Utc::now();
                (
                    "J'ancre l'entrée sur un repli vers le niveau clé, le stop au-delà de l'invalidation et \
                     les objectifs sur les prochaines zones de liquidité."
                        .into(),
                    json!({
                        "side": if long { "long" } else { "short" },
                        "entry": entry, "stop_loss": sl, "take_profits": [tp1, tp2],
                        "entry_window_start": now.to_rfc3339(),
                        "entry_window_end": (now + CDuration::hours(8)).to_rfc3339(),
                        "risk_reward": 2.0, "risk_pct": 0.5,
                        "confidence": 50 + (rng.f64() * 35.0) as i32,
                        "notes": "Mode démo : entrée limite sur repli, invalidation sous le dernier creux, alléger à TP1."
                    }),
                )
            }
            _ => (
                String::new(),
                json!("**Mode démo.** Je suis l'assistant Merces. Ajoutez votre clé Anthropic dans *Réglages → Données* pour des réponses réelles.\n\n- Cliquez un agent pour voir le travail d'un agent.\n- *Lancer l'analyse* démarre les 4 agents sur le symbole actif."),
            ),
        }
    }
}

impl LlmBackend for DemoLlm {
    async fn send(
        &self,
        req: &Request,
        on: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> LlmResult<Response> {
        let agent = req
            .system
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("Agent: "))
            .unwrap_or("assistant")
            .to_owned();
        let user = req
            .messages
            .last()
            .and_then(|m| m["content"].as_str())
            .unwrap_or_default()
            .to_owned();
        let mut rng = Rng::new(hash(&user) ^ Utc::now().timestamp_millis() as u64);
        let (thinking, answer) = self.answer(&agent, &user, &mut rng);
        if agent == "research" {
            on(StreamEvent::ToolCall {
                name: "web_search".into(),
                input: json!({"query": "market news today (demo)"}),
            });
            tokio::time::sleep(self.pace * 10).await;
            on(StreamEvent::SearchResults(8));
        }
        for w in thinking.split_inclusive(' ') {
            on(StreamEvent::Thinking(w.to_owned()));
            tokio::time::sleep(self.pace).await;
        }
        let text = match answer {
            Value::String(s) => s,
            v => v.to_string(),
        };
        let step = if agent == "assistant" { 6 } else { 48 };
        let chars: Vec<char> = text.chars().collect();
        for chunk in chars.chunks(step) {
            on(StreamEvent::Text(chunk.iter().collect()));
            tokio::time::sleep(self.pace / 2).await;
        }
        let usage = Usage {
            input_tokens: (req.system.len() + user.len()) as u64 / 4,
            output_tokens: (thinking.len() + text.len()) as u64 / 4,
            ..Default::default()
        };
        Ok(Response {
            content: vec![
                json!({"type": "thinking", "thinking": thinking}),
                json!({"type": "text", "text": text}),
            ],
            stop_reason: "end_turn".into(),
            model: format!("{} (demo)", req.model),
            usage,
        })
    }
}

/// Real or demo LLM, chosen at runtime.
#[derive(Clone)]
pub enum AnyLlm {
    Claude(Claude),
    Demo(DemoLlm),
}

impl LlmBackend for AnyLlm {
    async fn send(
        &self,
        req: &Request,
        on: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> LlmResult<Response> {
        match self {
            AnyLlm::Claude(c) => c.send(req, on).await,
            AnyLlm::Demo(d) => d.send(req, on).await,
        }
    }
}

/// Real or demo market, chosen at runtime.
#[derive(Clone)]
pub enum AnyMarket {
    Real(Market),
    Demo(DemoMarket),
}

impl MarketSource for AnyMarket {
    async fn candles(
        &self,
        s: &SymbolConfig,
        tf: Timeframe,
        limit: usize,
    ) -> MarketResult<Vec<Candle>> {
        match self {
            AnyMarket::Real(m) => m.candles(s, tf, limit).await,
            AnyMarket::Demo(m) => m.candles(s, tf, limit).await,
        }
    }
    async fn last_price(&self, s: &SymbolConfig) -> MarketResult<f64> {
        match self {
            AnyMarket::Real(m) => m.last_price(s).await,
            AnyMarket::Demo(m) => m.last_price(s).await,
        }
    }
    async fn news(&self, s: &SymbolConfig, limit: usize) -> MarketResult<Vec<NewsItem>> {
        match self {
            AnyMarket::Real(m) => m.news(s, limit).await,
            AnyMarket::Demo(m) => m.news(s, limit).await,
        }
    }
}

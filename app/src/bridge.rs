//! Runs the async engine on its own Tokio thread and connects it to Bevy:
//! commands go down an unbounded channel, events come back on a crossbeam
//! channel drained once per frame.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use bevy::prelude::*;
use crossbeam_channel::{Receiver, unbounded};
use merces_agents::demo::{DemoLlm, DemoMarket};
use merces_agents::{Command, Ctx};
use merces_core::{Candle, EngineEvent, SymbolConfig, Timeframe};
use merces_llm::{Claude, LlmBackend, Request, Response, StreamEvent};
use merces_market::{Market, MarketSource, NewsItem};
use merces_store::Store;
use tokio::sync::mpsc::UnboundedSender;

use crate::config::{self, AppConfig, Secrets};
use crate::state::Hub;

/// Real or simulated Claude, switchable at runtime.
pub struct SwitchLlm {
    pub real: Claude,
    pub demo: DemoLlm,
    pub demo_on: AtomicBool,
}

impl LlmBackend for SwitchLlm {
    async fn send(
        &self,
        req: &Request,
        on: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> merces_llm::Result<Response> {
        if self.demo_on.load(Ordering::Relaxed) {
            self.demo.send(req, on).await
        } else {
            self.real.send(req, on).await
        }
    }
}

/// Real or simulated market data, switchable at runtime.
pub struct SwitchMarket {
    pub real: Market,
    pub demo: DemoMarket,
    pub demo_on: AtomicBool,
}

impl MarketSource for SwitchMarket {
    async fn candles(
        &self,
        s: &SymbolConfig,
        tf: Timeframe,
        limit: usize,
    ) -> merces_market::Result<Vec<Candle>> {
        if self.demo_on.load(Ordering::Relaxed) {
            self.demo.candles(s, tf, limit).await
        } else {
            self.real.candles(s, tf, limit).await
        }
    }
    async fn last_price(&self, s: &SymbolConfig) -> merces_market::Result<f64> {
        if self.demo_on.load(Ordering::Relaxed) {
            self.demo.last_price(s).await
        } else {
            self.real.last_price(s).await
        }
    }
    async fn news(&self, s: &SymbolConfig, limit: usize) -> merces_market::Result<Vec<NewsItem>> {
        if self.demo_on.load(Ordering::Relaxed) {
            self.demo.news(s, limit).await
        } else {
            self.real.news(s, limit).await
        }
    }
}

#[derive(Resource)]
pub struct Bridge {
    cmd: UnboundedSender<Command>,
    events: Receiver<EngineEvent>,
    llm: Arc<SwitchLlm>,
    market: Arc<SwitchMarket>,
    rt: Arc<tokio::runtime::Runtime>,
}

impl Bridge {
    pub fn start(cfg: &AppConfig, secrets: &Secrets) -> Self {
        let (ev_tx, ev_rx) = unbounded();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let llm = Arc::new(SwitchLlm {
            real: Claude::new(secrets.anthropic_key()),
            demo: DemoLlm::default(),
            demo_on: AtomicBool::new(cfg.demo_mode),
        });
        let market = Arc::new(SwitchMarket {
            real: Market::new(secrets.market_keys()),
            demo: DemoMarket,
            demo_on: AtomicBool::new(cfg.demo_mode),
        });
        let db = config::data_dir().join("merces.sqlite");
        let store = Store::open(&db).unwrap_or_else(|e| {
            tracing::error!("database {}: {e}; using memory", db.display());
            Store::in_memory().expect("in-memory database")
        });
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("merces-engine")
                .enable_all()
                .build()
                .expect("tokio runtime"),
        );
        let ctx = Ctx {
            llm: llm.clone(),
            market: market.clone(),
            store: Arc::new(store),
            sink: Arc::new(move |e| {
                let _ = ev_tx.send(e);
            }),
            cfg: Arc::new(RwLock::new(cfg.engine.clone())),
        };
        rt.spawn(merces_agents::run(ctx, cmd_rx));
        let b = Self {
            cmd: cmd_tx,
            events: ev_rx,
            llm,
            market,
            rt,
        };
        b.send(Command::Focus(cfg.active_symbol.clone()));
        b
    }

    pub fn send(&self, c: Command) {
        let _ = self.cmd.send(c);
    }

    /// Push config, keys and mode to the running engine.
    pub fn sync(&self, cfg: &AppConfig, secrets: &Secrets) {
        self.llm.demo_on.store(cfg.demo_mode, Ordering::Relaxed);
        self.market.demo_on.store(cfg.demo_mode, Ordering::Relaxed);
        self.llm.real.set_key(secrets.anthropic_key());
        self.market.real.set_keys(secrets.market_keys());
        self.send(Command::Configure(cfg.engine.clone()));
    }

    pub fn has_claude_key(&self) -> bool {
        self.llm.real.has_key()
    }

    /// Symbol search on the providers (runs on the engine runtime).
    pub fn search(&self, query: String) -> Receiver<Vec<merces_core::SymbolInfo>> {
        let (tx, rx) = unbounded();
        let m = self.market.real.clone();
        self.rt.spawn(async move {
            let _ = tx.send(m.search(&query).await.unwrap_or_default());
        });
        rx
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
    }
}

/// Drain engine events into the [`Hub`].
pub fn pump(bridge: Res<Bridge>, mut hub: ResMut<Hub>, cfg: Res<AppConfig>, time: Res<Time>) {
    hub.now = time.elapsed_secs_f64();
    let now = hub.now;
    hub.toasts.retain(|t| t.until > now);
    // Bounded per frame so a burst never stalls rendering.
    for e in bridge.events.try_iter().take(2_000) {
        hub.apply(e, &cfg.active_symbol);
    }
}

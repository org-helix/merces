//! Built-in frame benchmark, used to measure performance before and after a change.
//!
//! ```sh
//! MERCES_HOME=/tmp/merces-bench MERCES_BENCH=20 cargo run --release
//! MERCES_HOME=/tmp/merces-bench MERCES_BENCH=20 MERCES_BENCH_RUN=1 cargo run --release
//! ```
//!
//! `MERCES_BENCH=<seconds>` records every frame time after a 3 s warm-up, prints
//! a summary (average FPS, p50/p99/max frame time, frames over 33 ms, time to
//! the first frame) and quits. `MERCES_BENCH_RUN=1` keeps a demo analysis
//! running for the whole measurement. Process CPU time is best read from the
//! outside (`/usr/bin/time -l` on macOS, `/usr/bin/time -v` on Linux).

use std::time::{Duration, Instant};

use bevy::prelude::*;
use merces_agents::Command;

use crate::bridge::Bridge;
use crate::config::AppConfig;
use crate::state::Hub;

const WARMUP: Duration = Duration::from_secs(3);

/// When the process started, for the time-to-first-frame figure.
#[derive(Resource, Clone, Copy)]
pub struct Started(pub Instant);

#[derive(Resource)]
struct Bench {
    length: Duration,
    keep_running: bool,
    first_frame: Option<Duration>,
    measuring_since: Option<Instant>,
    frames: Vec<f32>,
}

pub struct PerfPlugin;

impl Plugin for PerfPlugin {
    fn build(&self, app: &mut App) {
        let Some(secs) = std::env::var("MERCES_BENCH")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        else {
            return;
        };
        app.insert_resource(Bench {
            length: Duration::from_secs(secs.max(1)),
            keep_running: std::env::var_os("MERCES_BENCH_RUN").is_some(),
            first_frame: None,
            measuring_since: None,
            frames: Vec::with_capacity(secs as usize * 400),
        })
        .add_systems(Last, record);
    }
}

fn record(
    started: Res<Started>,
    real: Res<Time<Real>>,
    mut bench: ResMut<Bench>,
    hub: Res<Hub>,
    cfg: Res<AppConfig>,
    bridge: Res<Bridge>,
    mut exit: MessageWriter<AppExit>,
) {
    let b = &mut *bench;
    let first = *b.first_frame.get_or_insert_with(|| started.0.elapsed());
    if b.keep_running && !hub.symbol(&cfg.active_symbol).is_some_and(|v| v.running.is_some()) {
        bridge.send(Command::Run(cfg.active_symbol.clone()));
    }
    let now = Instant::now();
    let Some(since) = b.measuring_since else {
        if started.0.elapsed() >= first + WARMUP {
            b.measuring_since = Some(now);
        }
        return;
    };
    b.frames.push(real.delta_secs() * 1000.0);
    if now - since < b.length {
        return;
    }
    let mut sorted = b.frames.clone();
    sorted.sort_by(f32::total_cmp);
    let pct = |p: f32| sorted[((sorted.len() - 1) as f32 * p).round() as usize];
    let wall = (now - since).as_secs_f32();
    println!(
        "merces-bench frames={} avg_fps={:.1} p50_ms={:.2} p99_ms={:.2} max_ms={:.2} over_33ms={} first_frame_ms={} analysis={}",
        sorted.len(),
        sorted.len() as f32 / wall,
        pct(0.5),
        pct(0.99),
        sorted.last().copied().unwrap_or_default(),
        sorted.iter().filter(|ms| **ms > 33.3).count(),
        first.as_millis(),
        b.keep_running,
    );
    exit.write(AppExit::Success);
}

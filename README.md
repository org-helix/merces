# Merces

**Four AI agents read the markets and hand you one trading plan a day, or tell you to sit it out.**

Merces is a 3D dashboard of AI trading agents from [Helix](https://org-helix.github.io/helix-website/). Four small pixel-art agents work as a chain, like neurons in a network: one researches the market, one reads the charts, one decides, one writes the plan. You watch them hand work to each other, read their reasoning as it streams, and follow every plan on paper.

Like every Helix app, Merces is free and open source. It is a single native app for Windows, Linux and macOS, written in Rust with [Bevy](https://bevyengine.org) and [egui](https://github.com/emilk/egui). On first launch it runs in a **demo mode** that needs no key, no account and no connection.

> Decision support, not financial advice. Merces never places an order: every plan is tracked on paper (triggered, take profit, stop loss, expired, result in R).

![SCREENSHOT_OVERVIEW](docs/screenshots/overview.png)

<p align="center">
  <img src="docs/screenshots/agent.png" width="49%" alt="SCREENSHOT_AGENT">
  <img src="docs/screenshots/journal.png" width="49%" alt="SCREENSHOT_JOURNAL">
</p>

<details>
<summary>More screenshots</summary>

![SCREENSHOT_SYMBOLS](docs/screenshots/symbols.png)

![SCREENSHOT_SETTINGS](docs/screenshots/settings.png)

</details>

## Install

There is no packaged release yet. Build Merces from source with Rust 1.95 or newer:

```sh
git clone https://github.com/org-helix/merces.git
cd merces
cargo run --release
```

On Linux, building needs `libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev libdbus-1-dev pkg-config` (see `.github/workflows/ci.yml`).

## Features

- **Four agents in a chain**, fed by the active symbol (the input node on the left):
  1. **Research** reads the markets, the news and the economic calendar (Claude web search, Finnhub news), then gives the bias for the day and the week.
  2. **Chart analysis** reads the 1D, 4h, 1h and 15m charts. The indicators (EMA, RSI, MACD, ATR, Bollinger, VWAP, structure, support and resistance) are computed in Rust. The agent confirms or contradicts the research.
  3. **Decision**: buy, sell or **do nothing today**. On "do nothing", the chain stops there.
  4. **Execution** writes the plan: entry, time window, stop loss, TP1 to TP3, risk/reward, risk and confidence. Guardrails in Rust check the plan; an invalid plan goes back to the agent once, then is rejected.
- **Assistant**: a separate agent (the guide's hat, above the network) helps with the app, answers trading questions and can suggest symbols. Nothing is added until you confirm.
- **3D network**: drag to orbit (the rotation keeps its momentum), scroll to zoom. Click an agent and it waves, the camera flies to it and its section unfolds in the left drawer. Drag a node to move it; the network settles around it, with every link the same length. **Esc** or a click on empty space returns to the overview.
- **Side drawers**: fixed on the window's edges, each one rolls in to a thin strip and out again; every section folds and unfolds.
  - Left, **Analyses**: one section per agent. Its header shows the agent's state and latest result. Unfolded, **Live** shows the activity, the hand-offs and the reasoning as it streams, then becomes **Today** with the day's reports once the analysis ends; **History** lists every result of that agent.
  - Right, **Follow-up**: **Signals**, **Assistant**, **Symbols** and **Settings**.
- **Badges** float next to each agent: the day's results, coloured by outcome. Hover for the summary, click to open the report.
- **Animations**: agents move according to their state (idle, working, done, "nothing today", error) and react to hover, clicks, hand-offs (an envelope flies from one to the next) and plans issued or rejected.
- **Top bar**: it is also the title bar (drag to move the window, double-click to maximise). It holds the active symbol and its price, the run button (hover for the next scheduled run), and **Settings**, **Signals** and **Symbols**, which unfold their section in the right drawer.
- **Symbols**: search, add, daily run time (UTC) and risk limits per symbol.
- **Signals**: plans tracked on paper, win rate, cumulative result in R, CSV export.
- **Settings**:
  - *Appearance*: dark, light or system theme, accent, font, text size, 3D quality (low, medium, high), language. Under **Advanced**: heading font, FPS cap, network attraction and link distance, reduced motion, eco mode when the window is not focused.
  - *Agents*: model, effort and instructions for each agent, daily budget.
  - *Data* (keys, demo mode), *Risk*, *About*.
- **Languages**: French (default) and English, under **Settings → Appearance**.
- **Themes**: the 3D sky stays at night in both themes; the panels follow the chosen theme.

### Data sources

Demo mode simulates Claude and the market. To use real data, open **Settings → Data** and add the keys you need:

| Key | Used for |
|---|---|
| Anthropic | the five agents (Claude) |
| none | Binance (crypto) |
| Twelve Data | forex, indices, gold |
| Alpaca (id + secret) | US stocks |
| Finnhub (optional) | news |

## Privacy

- In demo mode, Merces makes no network request.
- With real data, Merces talks only to the services whose keys you entered: Anthropic for the agents, and Binance, Twelve Data, Alpaca and Finnhub for read-only market data. Each service's own privacy policy applies to what it receives.
- No Helix account, no ads, no analytics, no tracking.
- Keys are stored in the system keychain: Windows Credential Manager, macOS Keychain or Secret Service on Linux. When no keychain exists, they go to a private file, and the app says so.
- Analyses, reports, signals and costs stay on the computer, in a local SQLite database: `%APPDATA%\Helix\Merces` (Windows), `~/Library/Application Support/org.Helix.Merces` (macOS), `~/.config/merces` and `~/.local/share/merces` (Linux).

## Development

```sh
cargo run --features dev           # fast iteration (dynamic linking of Bevy)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

The tests cover the indicators (against reference values), the plan guardrails and the paper-tracking state machine, the SSE parser, the JSON schemas, the SQLite store, the whole pipeline with a scripted LLM (order 1 → 2 → 3 → 4, stop on "do nothing"), WCAG AA contrast of the theme, and the network layout (equal link lengths, balanced spread on all three axes, tunable attraction and distance, a released node settles back, reduced motion freezes it).

| Crate | What it holds |
|---|---|
| `crates/merces-core` | Domain types, plan guardrails, paper tracking |
| `crates/merces-ta` | Technical indicators and compact summaries for the LLM |
| `crates/merces-market` | Binance, Twelve Data, Alpaca, Finnhub (read only) |
| `crates/merces-llm` | Claude client over raw HTTP: SSE streaming, structured outputs, prompt caching, fallbacks, `pause_turn` resumption |
| `crates/merces-store` | SQLite: analyses, messages, reports, signals, costs |
| `crates/merces-agents` | Prompts, schemas, pipeline, scheduler, assistant, demo mode |
| `app` | Bevy app: 3D scene (spring network, WESL star-field shaders, voxel characters and procedural animation), frameless window, tokio ↔ Bevy bridge, Helix theme, egui panels |

### Performance

```sh
MERCES_HOME=/tmp/merces-bench MERCES_BENCH=20 cargo run --release                     # idle
MERCES_HOME=/tmp/merces-bench MERCES_BENCH=20 MERCES_BENCH_RUN=1 cargo run --release  # during a demo analysis
```

`MERCES_BENCH=<s>` records every frame after a 3 s warm-up, prints the average FPS, p50/p99/max frame time, frames over 33 ms and the time to the first frame, then quits. `MERCES_HOME` keeps the configuration and the database of the measurement apart. Read CPU time from the outside (`/usr/bin/time -l` on macOS, `-v` on Linux).

### Screenshots

The images in `docs/screenshots` come from the app itself. `MERCES_SHOT=<file.png>` saves a capture of the window after `MERCES_SHOT_AFTER` seconds (default 6) and quits. `MERCES_SHOT_AGENT=research|chart|decision|execution|assistant` selects an agent first, `MERCES_SHOT_UI=settings|symbols|journal` unfolds that section of the right drawer, and `MERCES_SHOT_RUN=1` starts a demo analysis. Retake them after a visual change, in English and demo mode, with a `merces.toml` in `MERCES_HOME` that holds:

```toml
demo_mode = true
disclaimer_seen = true

[engine]
lang = "en"
```

## Design

Merces uses the Helix colour palette, as in [netlix](https://github.com/org-helix/netlix), on top of the Dusk DS structure: ink and Pale Amber, Red `#B24E30` as the action colour, Bricolage Grotesque titles, Geist text and no drop shadows. All colours live in `app/src/theme.rs`, and a unit test checks WCAG AA contrast for both themes. The characters, the network and the sky are drawn by the code; there is no external 3D model, image or sound (see [`app/assets/ASSETS.md`](app/assets/ASSETS.md)).

## Credits

- **[Helix](https://github.com/org-helix)** designs, builds and maintains Merces.
- **Characters**: the agents' bodies are a voxel version of the Claude Code pixel mascot by Anthropic. Anthropic is not affiliated with this project; the mascot and the Claude name remain Anthropic's property.
- **Fonts**: [Bricolage Grotesque](https://github.com/ateliertriay/bricolage) and [Geist](https://github.com/vercel/geist-font), bundled under the SIL Open Font License ([`app/assets/fonts`](app/assets/fonts)).
- **Libraries**: [Bevy](https://bevyengine.org), [egui](https://github.com/emilk/egui), [tokio](https://tokio.rs), [reqwest](https://github.com/seanmonstar/reqwest) and [rusqlite](https://github.com/rusqlite/rusqlite), each under its own licence (MIT or Apache 2.0).

Questions, ideas and bug reports are welcome in the [issues](https://github.com/org-helix/merces/issues). Before sending a change, read the [Helix contributing guide](https://github.com/org-helix/.github/blob/main/CONTRIBUTING.md); to report a security problem, follow the [security policy](https://github.com/org-helix/.github/blob/main/SECURITY.md).

## License

Merces is free software, released under the [GNU General Public License, version 3](LICENSE). You may use, study, share and change it; versions you distribute must stay under the same license. The bundled fonts keep their own license (SIL Open Font License).

> No tracking, no ads, no lock-in. Built for and with the communities that use it.

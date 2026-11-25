//! Technical indicators, computed in Rust so the chart agent receives exact numbers
//! instead of reading raw candles. Every series has the input's length; values that
//! are not defined yet (warm-up) are `NaN`.

use merces_core::{Candle, Timeframe};
use serde::Serialize;

pub fn sma(src: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; src.len()];
    if n == 0 || src.len() < n {
        return out;
    }
    let mut sum: f64 = src[..n].iter().sum();
    out[n - 1] = sum / n as f64;
    for i in n..src.len() {
        sum += src[i] - src[i - n];
        out[i] = sum / n as f64;
    }
    out
}

/// EMA seeded with the SMA of the first `n` values.
pub fn ema(src: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; src.len()];
    if n == 0 || src.len() < n {
        return out;
    }
    let k = 2.0 / (n as f64 + 1.0);
    let mut prev = src[..n].iter().sum::<f64>() / n as f64;
    out[n - 1] = prev;
    for i in n..src.len() {
        prev = src[i] * k + prev * (1.0 - k);
        out[i] = prev;
    }
    out
}

/// Wilder's RSI.
pub fn rsi(src: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; src.len()];
    if n == 0 || src.len() <= n {
        return out;
    }
    let (mut gain, mut loss) = (0.0, 0.0);
    for i in 1..=n {
        let d = src[i] - src[i - 1];
        if d > 0.0 { gain += d } else { loss -= d }
    }
    gain /= n as f64;
    loss /= n as f64;
    let value = |g: f64, l: f64| {
        if l == 0.0 {
            100.0
        } else {
            100.0 - 100.0 / (1.0 + g / l)
        }
    };
    out[n] = value(gain, loss);
    for i in n + 1..src.len() {
        let d = src[i] - src[i - 1];
        gain = (gain * (n as f64 - 1.0) + d.max(0.0)) / n as f64;
        loss = (loss * (n as f64 - 1.0) + (-d).max(0.0)) / n as f64;
        out[i] = value(gain, loss);
    }
    out
}

pub struct Macd {
    pub macd: Vec<f64>,
    pub signal: Vec<f64>,
    pub hist: Vec<f64>,
}

pub fn macd(src: &[f64], fast: usize, slow: usize, signal: usize) -> Macd {
    let f = ema(src, fast);
    let s = ema(src, slow);
    let line: Vec<f64> = f.iter().zip(&s).map(|(a, b)| a - b).collect();
    // Signal EMA over the defined part of the MACD line only.
    let start = line
        .iter()
        .position(|v| v.is_finite())
        .unwrap_or(line.len());
    let mut sig = vec![f64::NAN; src.len()];
    let tail = ema(&line[start..], signal);
    sig[start..].copy_from_slice(&tail);
    let hist = line.iter().zip(&sig).map(|(a, b)| a - b).collect();
    Macd {
        macd: line,
        signal: sig,
        hist,
    }
}

/// Wilder's ATR.
#[allow(clippy::needless_range_loop)] // `i` indexes both the candles and the output.
pub fn atr(c: &[Candle], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; c.len()];
    if n == 0 || c.len() <= n {
        return out;
    }
    let tr = |i: usize| {
        let prev = c[i - 1].c;
        (c[i].h - c[i].l)
            .max((c[i].h - prev).abs())
            .max((c[i].l - prev).abs())
    };
    let mut a = (1..=n).map(tr).sum::<f64>() / n as f64;
    out[n] = a;
    for i in n + 1..c.len() {
        a = (a * (n as f64 - 1.0) + tr(i)) / n as f64;
        out[i] = a;
    }
    out
}

pub struct Bands {
    pub mid: Vec<f64>,
    pub upper: Vec<f64>,
    pub lower: Vec<f64>,
}

pub fn bollinger(src: &[f64], n: usize, k: f64) -> Bands {
    let mid = sma(src, n);
    let mut upper = vec![f64::NAN; src.len()];
    let mut lower = vec![f64::NAN; src.len()];
    for i in n.saturating_sub(1)..src.len() {
        if !mid[i].is_finite() {
            continue;
        }
        let w = &src[i + 1 - n..=i];
        let var = w.iter().map(|x| (x - mid[i]).powi(2)).sum::<f64>() / n as f64;
        let sd = var.sqrt();
        upper[i] = mid[i] + k * sd;
        lower[i] = mid[i] - k * sd;
    }
    Bands { mid, upper, lower }
}

/// Volume-weighted average price over the whole slice (pass a session's candles).
pub fn vwap(c: &[Candle]) -> f64 {
    let (pv, v) = c.iter().fold((0.0, 0.0), |(pv, v), k| {
        let typical = (k.h + k.l + k.c) / 3.0;
        (pv + typical * k.v, v + k.v)
    });
    if v > 0.0 { pv / v } else { f64::NAN }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Swing {
    pub index: usize,
    pub price: f64,
    pub high: bool,
}

/// Fractal swing points: a high (low) greater (lower) than the `k` bars on each side.
pub fn swings(c: &[Candle], k: usize) -> Vec<Swing> {
    let mut out = Vec::new();
    if c.len() < 2 * k + 1 {
        return out;
    }
    for i in k..c.len() - k {
        let w = &c[i - k..=i + k];
        if w.iter().enumerate().all(|(j, x)| j == k || x.h < c[i].h) {
            out.push(Swing {
                index: i,
                price: c[i].h,
                high: true,
            });
        }
        if w.iter().enumerate().all(|(j, x)| j == k || x.l > c[i].l) {
            out.push(Swing {
                index: i,
                price: c[i].l,
                high: false,
            });
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Level {
    pub price: f64,
    /// How many swing points merged into this level.
    pub touches: u32,
}

/// Groups swing points closer than `tol` into levels, strongest first.
pub fn levels(sw: &[Swing], tol: f64) -> Vec<Level> {
    let mut prices: Vec<f64> = sw.iter().map(|s| s.price).collect();
    prices.sort_by(f64::total_cmp);
    let mut out: Vec<(f64, u32)> = Vec::new();
    for p in prices {
        match out.last_mut() {
            Some((sum_avg, n)) if (p - *sum_avg).abs() <= tol => {
                *sum_avg = (*sum_avg * *n as f64 + p) / (*n as f64 + 1.0);
                *n += 1;
            }
            _ => out.push((p, 1)),
        }
    }
    let mut lv: Vec<Level> = out
        .into_iter()
        .map(|(price, touches)| Level { price, touches })
        .collect();
    lv.sort_by_key(|l| std::cmp::Reverse(l.touches));
    lv
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Structure {
    /// Higher highs and higher lows.
    Uptrend,
    /// Lower highs and lower lows.
    Downtrend,
    Range,
}

/// Market structure from the last two swing highs and lows.
pub fn structure(sw: &[Swing]) -> Structure {
    let highs: Vec<f64> = sw.iter().filter(|s| s.high).map(|s| s.price).collect();
    let lows: Vec<f64> = sw.iter().filter(|s| !s.high).map(|s| s.price).collect();
    if highs.len() < 2 || lows.len() < 2 {
        return Structure::Range;
    }
    let hh = highs[highs.len() - 1] > highs[highs.len() - 2];
    let hl = lows[lows.len() - 1] > lows[lows.len() - 2];
    match (hh, hl) {
        (true, true) => Structure::Uptrend,
        (false, false) => Structure::Downtrend,
        _ => Structure::Range,
    }
}

fn last(v: &[f64]) -> Option<f64> {
    v.last().copied().filter(|x| x.is_finite())
}

fn round(x: f64, sig: i32) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let mag = 10f64.powi(sig - 1 - x.abs().log10().floor() as i32);
    (x * mag).round() / mag
}

/// Compact, LLM-friendly summary of one timeframe.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaSummary {
    pub timeframe: &'static str,
    pub candles: usize,
    pub last_close: f64,
    pub change_pct_20: Option<f64>,
    pub ema20: Option<f64>,
    pub ema50: Option<f64>,
    pub ema200: Option<f64>,
    pub rsi14: Option<f64>,
    pub macd_hist: Option<f64>,
    pub macd_cross: Option<&'static str>,
    pub atr14: Option<f64>,
    pub bb_upper: Option<f64>,
    pub bb_lower: Option<f64>,
    pub vwap_recent: Option<f64>,
    pub structure: Structure,
    pub supports: Vec<Level>,
    pub resistances: Vec<Level>,
    pub recent_high: f64,
    pub recent_low: f64,
    /// Last 12 closes, oldest first.
    pub last_closes: Vec<f64>,
}

pub fn summarize(tf: Timeframe, c: &[Candle]) -> Option<TaSummary> {
    let close: Vec<f64> = c.iter().map(|k| k.c).collect();
    let last_close = *close.last()?;
    let r = |x: Option<f64>| x.map(|v| round(v, 6));
    let m = macd(&close, 12, 26, 9);
    let cross = {
        let n = m.hist.len();
        if n >= 2 && m.hist[n - 1].is_finite() && m.hist[n - 2].is_finite() {
            match (m.hist[n - 2] <= 0.0, m.hist[n - 1] > 0.0) {
                (true, true) => Some("bullish_cross"),
                (false, false) => Some("bearish_cross"),
                _ => None,
            }
        } else {
            None
        }
    };
    let a = atr(c, 14);
    let atr_last = last(&a);
    let sw = swings(c, 3);
    let tol = atr_last.unwrap_or(last_close * 0.005) * 0.5;
    let lv = levels(&sw, tol);
    let mut supports: Vec<Level> = lv
        .iter()
        .filter(|l| l.price < last_close)
        .copied()
        .take(4)
        .collect();
    let mut resistances: Vec<Level> = lv
        .iter()
        .filter(|l| l.price >= last_close)
        .copied()
        .take(4)
        .collect();
    for l in supports.iter_mut().chain(resistances.iter_mut()) {
        l.price = round(l.price, 6);
    }
    supports.sort_by(|a, b| b.price.total_cmp(&a.price));
    resistances.sort_by(|a, b| a.price.total_cmp(&b.price));
    let window = &c[c.len().saturating_sub(50)..];
    let bb = bollinger(&close, 20, 2.0);
    Some(TaSummary {
        timeframe: tf.label(),
        candles: c.len(),
        last_close,
        change_pct_20: (close.len() > 20)
            .then(|| round((last_close / close[close.len() - 21] - 1.0) * 100.0, 4)),
        ema20: r(last(&ema(&close, 20))),
        ema50: r(last(&ema(&close, 50))),
        ema200: r(last(&ema(&close, 200))),
        rsi14: r(last(&rsi(&close, 14))),
        macd_hist: r(last(&m.hist)),
        macd_cross: cross,
        atr14: r(atr_last),
        bb_upper: r(last(&bb.upper)),
        bb_lower: r(last(&bb.lower)),
        vwap_recent: r(Some(vwap(&c[c.len().saturating_sub(24)..])).filter(|v| v.is_finite())),
        structure: structure(&sw),
        supports,
        resistances,
        recent_high: window.iter().map(|k| k.h).fold(f64::MIN, f64::max),
        recent_low: window.iter().map(|k| k.l).fold(f64::MAX, f64::min),
        last_closes: close[close.len().saturating_sub(12)..].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn sma_and_ema_reference_values() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let s = sma(&x, 3);
        assert!(s[1].is_nan());
        assert!(close(s[2], 2.0) && close(s[4], 4.0));
        let e = ema(&x, 3);
        // seed 2.0, k = 0.5 → 3.0, 4.0
        assert!(close(e[2], 2.0) && close(e[3], 3.0) && close(e[4], 4.0));
    }

    #[test]
    fn rsi_reference_values() {
        // Wilder's classic 14-period example (StockCharts: 70.53 / 66.32 with
        // unrounded closes; these 2-decimal closes land within 0.1).
        let x = [
            44.34, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84, 46.08, 45.89, 46.03,
            45.61, 46.28, 46.28, 46.00, 46.03, 46.41, 46.22, 45.64,
        ];
        let r = rsi(&x, 14);
        assert!((r[14] - 70.53).abs() < 0.1, "{}", r[14]);
        assert!((r[15] - 66.32).abs() < 0.1, "{}", r[15]);
        assert!(r.iter().take(14).all(|v| v.is_nan()));
    }

    #[test]
    fn rsi_is_100_on_steady_rise() {
        let x: Vec<f64> = (0..30).map(f64::from).collect();
        assert!(close(*rsi(&x, 14).last().unwrap(), 100.0));
    }

    fn candle(i: i64, c: f64) -> Candle {
        Candle {
            t: i,
            o: c,
            h: c + 1.0,
            l: c - 1.0,
            c,
            v: 10.0,
        }
    }

    #[test]
    fn atr_of_constant_range() {
        let c: Vec<Candle> = (0..30).map(|i| candle(i, 100.0)).collect();
        assert!(close(*atr(&c, 14).last().unwrap(), 2.0));
    }

    #[test]
    fn structure_detects_uptrend() {
        // Zig-zag going up.
        let p = [
            10.0, 11., 12., 11., 10.5, 11.5, 13., 14., 13., 12., 12.5, 14., 15., 16., 15., 14.,
            14.5, 16., 17., 18., 17., 16.,
        ];
        let c: Vec<Candle> = p
            .iter()
            .enumerate()
            .map(|(i, v)| candle(i as i64, *v))
            .collect();
        assert_eq!(structure(&swings(&c, 2)), Structure::Uptrend);
    }

    #[test]
    fn summary_has_levels_on_both_sides() {
        let c: Vec<Candle> = (0..300)
            .map(|i| candle(i, 100.0 + 10.0 * (i as f64 / 9.0).sin()))
            .collect();
        let s = summarize(Timeframe::H1, &c).unwrap();
        assert!(s.ema200.is_some() && s.rsi14.is_some() && s.atr14.is_some());
        assert!(s.supports.iter().all(|l| l.price < s.last_close));
        assert!(s.resistances.iter().all(|l| l.price >= s.last_close));
    }
}

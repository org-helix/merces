//! Tiny Markdown renderer for the assistant (headings, lists, bold, inline code,
//! links). egui_commonmark targets a newer egui than bevy_egui, so this covers
//! what the assistant is asked to write.

use bevy_egui::egui::{self, FontFamily, FontId, RichText, text::LayoutJob};

use crate::theme::{self, Theme};

fn inline(ui: &egui::Ui, th: &Theme, text: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let size = 14.0 * th.scale;
    let normal = egui::TextFormat {
        font_id: FontId::new(size, FontFamily::Proportional),
        color: th.pal.body,
        ..Default::default()
    };
    let bold = egui::TextFormat {
        font_id: FontId::new(size, theme::semibold()),
        color: th.pal.text,
        ..Default::default()
    };
    let code = egui::TextFormat {
        font_id: FontId::new(13.0 * th.scale, FontFamily::Monospace),
        color: th.pal.text,
        background: th.pal.fill,
        ..Default::default()
    };
    let italic = egui::TextFormat {
        italics: true,
        ..normal.clone()
    };
    let mut rest = text;
    while !rest.is_empty() {
        let next = ["**", "`", "*"]
            .iter()
            .filter_map(|m| rest.find(m).map(|i| (i, *m)))
            .min_by_key(|(i, m)| (*i, std::cmp::Reverse(m.len())));
        let Some((i, marker)) = next else {
            job.append(rest, 0.0, normal.clone());
            break;
        };
        job.append(&rest[..i], 0.0, normal.clone());
        let after = &rest[i + marker.len()..];
        match after.find(marker) {
            Some(j) if j > 0 => {
                let fmt = match marker {
                    "**" => bold.clone(),
                    "`" => code.clone(),
                    _ => italic.clone(),
                };
                job.append(&after[..j], 0.0, fmt);
                rest = &after[j + marker.len()..];
            }
            _ => {
                job.append(marker, 0.0, normal.clone());
                rest = after;
            }
        }
    }
    job.wrap.max_width = ui.available_width();
    job
}

pub fn show(ui: &mut egui::Ui, th: &Theme, text: &str) {
    let mut in_code = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            ui.label(theme::mono(th, line));
            continue;
        }
        if trimmed.is_empty() {
            ui.add_space(4.0);
        } else if let Some(h) = trimmed
            .strip_prefix("### ")
            .or_else(|| trimmed.strip_prefix("## "))
        {
            ui.label(theme::title(th, h, 16.0));
        } else if let Some(h) = trimmed.strip_prefix("# ") {
            ui.label(theme::title(th, h, 18.0));
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("•").color(th.pal.muted));
                let job = inline(ui, th, item);
                ui.label(job);
            });
        } else if trimmed.len() > 2
            && trimmed.chars().next().is_some_and(|c| c.is_ascii_digit())
            && trimmed.contains(". ")
        {
            let (n, item) = trimmed.split_once(". ").unwrap_or(("", trimmed));
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{n}.")).color(th.pal.muted));
                let job = inline(ui, th, item);
                ui.label(job);
            });
        } else {
            let job = inline(ui, th, trimmed);
            ui.label(job);
        }
    }
}

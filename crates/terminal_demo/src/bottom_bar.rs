use std::time::Duration;

use chrono::{DateTime, Local};
use gpui::{Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Task, Window, div, px};
use gpui_component::{ActiveTheme as _, StyledExt as _, h_flex};

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct BottomBar {
    now: DateTime<Local>,
    /// Number of `render()` calls observed since the last FPS sample.
    frame_count: u32,
    /// Last time we re-computed `fps`. Sampling once per ~500ms keeps the
    /// readout legible without making the bar's text twitch every frame.
    last_fps_sample: DateTime<Local>,
    /// Last computed FPS value (rendered in the bar).
    fps: f32,
    _tick_task: Task<()>,
}

impl BottomBar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Tick once per second so the clock stays accurate even when nothing
        // else in the workspace causes a redraw.
        let _tick_task = cx.spawn_in(window, async move |this, window| {
            loop {
                window
                    .background_executor()
                    .timer(Duration::from_secs(1))
                    .await;
                if this
                    .update(window, |this, cx| {
                        this.now = Local::now();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        let now = Local::now();
        Self {
            now,
            frame_count: 0,
            last_fps_sample: now,
            fps: 0.0,
            _tick_task,
        }
    }
}

impl Render for BottomBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        // Each render counts as one painted frame for this view. Recompute FPS
        // every ~500ms so the readout updates at human-readable cadence.
        self.frame_count += 1;
        let now = Local::now();
        let elapsed_ms = (now - self.last_fps_sample).num_milliseconds();
        if elapsed_ms >= 500 {
            self.fps = (self.frame_count as f32) * 1000.0 / (elapsed_ms as f32);
            self.frame_count = 0;
            self.last_fps_sample = now;
        }
        // Drive the next paint so FPS keeps sampling. Without this, gpui only
        // redraws on demand and the counter would stall as soon as the user
        // stops interacting.
        window.request_animation_frame();

        let muted = theme.muted_foreground;
        let bullish = theme.chart_bullish;

        let connection = h_flex()
            .gap_1p5()
            .items_center()
            .child(div().size_2().rounded_full().bg(bullish))
            .child(div().text_xs().text_color(theme.foreground).child("Connected"))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("· market data · 12ms"),
            );

        let time_str = SharedString::from(self.now.format("%H:%M:%S").to_string());
        let date_str = SharedString::from(self.now.format("%a, %b %d %Y").to_string());
        let tz_str = SharedString::from(format!("UTC{}", self.now.format("%:z")));

        let clock = h_flex()
            .gap_2()
            .items_baseline()
            .child(div().text_xs().font_semibold().child(time_str))
            .child(div().text_xs().text_color(muted).child(date_str))
            .child(div().text_xs().text_color(muted).child(tz_str));

        let fps_color = if self.fps >= 50.0 {
            bullish
        } else if self.fps >= 25.0 {
            theme.chart_5
        } else {
            theme.chart_bearish
        };
        let fps_str = SharedString::from(format!("{:>4.0} fps", self.fps));

        let fps = h_flex()
            .gap_1p5()
            .items_center()
            .child(div().text_xs().text_color(muted).child("FPS"))
            .child(
                div()
                    .text_xs()
                    .font_semibold()
                    .text_color(fps_color)
                    .child(fps_str),
            );

        let version = div()
            .text_xs()
            .text_color(muted)
            .child(SharedString::from(format!("v{VERSION}")));

        let separator = || div().w(px(1.)).h(px(14.)).bg(theme.border);

        h_flex()
            .h(px(24.))
            .w_full()
            .px_3()
            .gap_3()
            .items_center()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.tab_bar)
            .child(connection)
            .child(div().flex_1())
            .child(clock)
            .child(separator())
            .child(fps)
            .child(separator())
            .child(version)
    }
}

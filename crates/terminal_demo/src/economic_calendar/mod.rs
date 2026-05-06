use chrono::NaiveDate;
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    calendar::{Calendar, CalendarEvent, CalendarState, Date},
    dock::{Panel, PanelEvent, TabPanel},
    h_flex, v_flex,
};
use std::collections::HashSet;

mod data;

use crate::panels::{Kind, LastFocusedTabPanel};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Impact {
    High,
    Medium,
    Low,
    Holiday,
}

impl Impact {
    fn label(self) -> &'static str {
        match self {
            Impact::High => "High",
            Impact::Medium => "Medium",
            Impact::Low => "Low",
            Impact::Holiday => "Holiday",
        }
    }

    fn dot_color(self, theme: &gpui_component::theme::Theme) -> gpui::Hsla {
        match self {
            Impact::High => theme.chart_bearish,
            Impact::Medium => theme.warning,
            Impact::Low => theme.chart_bullish,
            Impact::Holiday => theme.muted_foreground,
        }
    }

    const ALL: &'static [Impact] = &[
        Impact::High,
        Impact::Medium,
        Impact::Low,
        Impact::Holiday,
    ];
}

pub struct EconomicEvent {
    pub date: &'static str,
    pub time: &'static str,
    pub currency: &'static str,
    pub impact: Impact,
    pub title: &'static str,
    pub actual: &'static str,
    pub forecast: &'static str,
    pub previous: &'static str,
}

pub struct EconomicCalendarPanel {
    focus_handle: FocusHandle,
    parent_tab_panel: Option<gpui::WeakEntity<TabPanel>>,
    calendar: Entity<CalendarState>,
    calendar_open: bool,
    impact_filter: HashSet<Impact>,
    _calendar_subscription: Subscription,
}

impl EconomicCalendarPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let calendar = cx.new(|cx| {
            let mut state = CalendarState::new(window, cx);
            // Default to the snapshot date from the dataset so the panel opens with events visible.
            if let Ok(today) = NaiveDate::parse_from_str("2026-05-06", "%Y-%m-%d") {
                state.set_date(today, window, cx);
            }
            state
        });

        let _calendar_subscription =
            cx.subscribe(&calendar, |_this, _state, _ev: &CalendarEvent, cx| {
                cx.notify();
            });

        let mut impact_filter = HashSet::new();
        impact_filter.insert(Impact::High);
        impact_filter.insert(Impact::Medium);
        impact_filter.insert(Impact::Low);

        Self {
            focus_handle: cx.focus_handle(),
            parent_tab_panel: None,
            calendar,
            calendar_open: true,
            impact_filter,
            _calendar_subscription,
        }
    }

    fn toggle_calendar(&mut self, cx: &mut Context<Self>) {
        self.calendar_open = !self.calendar_open;
        cx.notify();
    }

    fn mark_focused(&self, cx: &mut App) {
        let Some(tab_panel) = self.parent_tab_panel.clone() else {
            return;
        };
        let global = cx.global::<LastFocusedTabPanel>().0.clone();
        *global.borrow_mut() = Some(tab_panel);
    }

    fn is_focused(&self, cx: &App) -> bool {
        let Some(mine) = self.parent_tab_panel.as_ref() else {
            return false;
        };
        let global = cx.global::<LastFocusedTabPanel>().0.borrow();
        global
            .as_ref()
            .map(|w| w.entity_id() == mine.entity_id())
            .unwrap_or(false)
    }

    fn toggle_impact(&mut self, impact: Impact, cx: &mut Context<Self>) {
        if !self.impact_filter.insert(impact) {
            self.impact_filter.remove(&impact);
        }
        cx.notify();
    }

    fn selected_date_str(&self, cx: &App) -> Option<String> {
        match self.calendar.read(cx).date() {
            Date::Single(Some(d)) => Some(d.format("%Y-%m-%d").to_string()),
            Date::Range(Some(start), _) => Some(start.format("%Y-%m-%d").to_string()),
            _ => None,
        }
    }
}

impl EventEmitter<PanelEvent> for EconomicCalendarPanel {}

impl Focusable for EconomicCalendarPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for EconomicCalendarPanel {
    fn panel_name(&self) -> &'static str {
        Kind::EconomicCalendar.id()
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        SharedString::from(Kind::EconomicCalendar.display())
    }

    fn on_added_to(
        &mut self,
        tab_panel: gpui::WeakEntity<TabPanel>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.parent_tab_panel = Some(tab_panel);
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            self.mark_focused(cx);
        }
    }
}

impl Render for EconomicCalendarPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let border = theme.border;
        let accent = theme.accent;
        let accent_fg = theme.accent_foreground;
        let bullish = theme.chart_bullish;
        let bearish = theme.chart_bearish;
        let ring_color = if self.is_focused(cx) {
            theme.ring
        } else {
            gpui::transparent_black()
        };

        // Filter rows by selected date + impact set.
        let selected = self.selected_date_str(cx);
        let filter = self.impact_filter.clone();
        let rows: Vec<&'static EconomicEvent> = data::EVENTS
            .iter()
            .filter(|e| match selected.as_deref() {
                Some(d) => e.date == d,
                None => true,
            })
            .filter(|e| filter.contains(&e.impact))
            .collect();

        let header_label = match selected.as_deref() {
            Some(d) => SharedString::from(format!("Events · {}", d)),
            None => SharedString::from("All events"),
        };
        let count_label = SharedString::from(format!("{} match", rows.len()));

        // Impact filter chips: each is a small button-styled toggle. Colored dot up front
        // shows the impact level; primary/outline variant shows on/off state.
        let mut chips = h_flex().gap_2().items_center();
        for &impact in Impact::ALL {
            let on = self.impact_filter.contains(&impact);
            let dot = impact.dot_color(theme);
            let label = impact.label();
            let id = SharedString::from(format!("impact-chip-{label}"));
            chips = chips.child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .px_2()
                    .py_1()
                    .rounded(gpui::px(4.))
                    .border_1()
                    .border_color(if on { dot } else { border })
                    .when(on, |s| s.bg(theme.muted))
                    .child(div().size_2().rounded_full().bg(dot))
                    .child(
                        Button::new(id)
                            .label(label)
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_impact(impact, cx);
                            })),
                    ),
            );
        }

        let calendar_open = self.calendar_open;
        let calendar_label = match selected.as_deref() {
            Some(d) => SharedString::from(format!("Calendar · {}", d)),
            None => SharedString::from("Calendar"),
        };
        let chevron = if calendar_open { "▾" } else { "▸" };

        let calendar_section = v_flex()
            .gap_2()
            .child(
                Button::new("toggle-calendar")
                    .small()
                    .ghost()
                    .label(SharedString::from(format!("{chevron}  {calendar_label}")))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_calendar(cx))),
            )
            .when(calendar_open, |s| s.child(Calendar::new(&self.calendar)));

        let body = v_flex()
            .w_full()
            .gap_3()
            .p_3()
            .child(calendar_section)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().text_xs().text_color(muted).child("Impact:"))
                    .child(chips),
            )
            .child(
                h_flex()
                    .items_baseline()
                    .gap_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(border)
                    .child(div().text_sm().font_semibold().child(header_label))
                    .child(div().flex_1())
                    .child(div().text_xs().text_color(muted).child(count_label)),
            )
            .child(
                // Column header row
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .text_xs()
                    .text_color(muted)
                    .border_b_1()
                    .border_color(border)
                    .child(div().w_16().child("Time"))
                    .child(div().w_12().child("Cur"))
                    .child(div().w_4().child(""))
                    .child(div().flex_1().child("Event"))
                    .child(div().w_16().text_right().child("Actual"))
                    .child(div().w_16().text_right().child("Forecast"))
                    .child(div().w_16().text_right().child("Previous")),
            )
            .child(if rows.is_empty() {
                div()
                    .py_6()
                    .text_sm()
                    .text_color(muted)
                    .child("No events for the selected date / impact filter.")
                    .into_any_element()
            } else {
                v_flex()
                    .gap_0()
                    .children(rows.into_iter().map(|e| {
                        let dot = e.impact.dot_color(theme);
                        let actual_color = match (e.actual, e.forecast) {
                            (a, f) if !a.is_empty() && !f.is_empty() => {
                                if let (Ok(av), Ok(fv)) = (parse_num(a), parse_num(f)) {
                                    if av >= fv { bullish } else { bearish }
                                } else {
                                    cx.theme().foreground
                                }
                            }
                            _ => cx.theme().foreground,
                        };
                        h_flex()
                            .px_2()
                            .py_1p5()
                            .gap_2()
                            .text_sm()
                            .border_b_1()
                            .border_color(border)
                            .child(
                                div()
                                    .w_16()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(e.time),
                            )
                            .child(
                                div()
                                    .w_12()
                                    .child(
                                        div()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded(gpui::px(3.))
                                            .bg(accent)
                                            .text_color(accent_fg)
                                            .text_xs()
                                            .child(e.currency),
                                    ),
                            )
                            .child(
                                div().w_4().flex().items_center().child(
                                    div().size_2().rounded_full().bg(dot),
                                ),
                            )
                            .child(div().flex_1().child(e.title))
                            .child(
                                div()
                                    .w_16()
                                    .text_right()
                                    .text_color(actual_color)
                                    .font_semibold()
                                    .child(if e.actual.is_empty() { "—" } else { e.actual }),
                            )
                            .child(
                                div()
                                    .w_16()
                                    .text_right()
                                    .text_color(muted)
                                    .child(if e.forecast.is_empty() { "—" } else { e.forecast }),
                            )
                            .child(
                                div()
                                    .w_16()
                                    .text_right()
                                    .text_color(muted)
                                    .child(if e.previous.is_empty() { "—" } else { e.previous }),
                            )
                    }))
                    .into_any_element()
            });

        // Outer scroll wrapper + focus border, mirroring panels.rs::Render for ContentPanel.
        div()
            .id("econ-calendar-panel-body")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev, _window, cx| this.mark_focused(cx)),
            )
            .size_full()
            .border_2()
            .border_color(ring_color)
            .child(
                div()
                    .id("econ-calendar-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .child(body),
            )
    }
}

/// Parse a ForexFactory value (e.g. "3.3%", "-17.2", "218K") into a comparable f64.
fn parse_num(s: &str) -> Result<f64, ()> {
    let s = s.trim();
    let mul = if let Some(rest) = s.strip_suffix('K') {
        return rest
            .replace(',', "")
            .parse::<f64>()
            .map(|v| v * 1_000.0)
            .map_err(|_| ());
    } else if let Some(rest) = s.strip_suffix('M') {
        return rest
            .replace(',', "")
            .parse::<f64>()
            .map(|v| v * 1_000_000.0)
            .map_err(|_| ());
    } else if let Some(rest) = s.strip_suffix('B') {
        return rest
            .replace(',', "")
            .parse::<f64>()
            .map(|v| v * 1_000_000_000.0)
            .map_err(|_| ());
    } else if let Some(rest) = s.strip_suffix('%') {
        rest
    } else {
        s
    };
    mul.replace(',', "").parse::<f64>().map_err(|_| ())
}

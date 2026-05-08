use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use gpui::{
    Action, App, AppContext as _, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable,
    Global, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    ParentElement as _, Pixels, Point, Render, ScrollWheelEvent, SharedString,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::{
    ActiveTheme as _, ElementExt as _, Sizable as _, StyledExt as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    chart::{CandlestickChart, nice_y_axis_gap},
    description_list::DescriptionList,
    dialog::DialogButtonProps,
    dock::{Panel, PanelEvent, PanelView, TabPanel, register_panel},
    h_flex,
    input::{Input, InputState},
    link::Link,
    menu::DropdownMenu as _,
    notification::Notification,
    plot::AXIS_GAP,
    text::{html, markdown},
    v_flex,
};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Watchlist,
    Chart,
    Details,
    NewsFeed,
    Portfolio,
    Notification,
    SmartMoney,
    AiChat,
    EconomicCalendar,
    Position,
    Execution,
    Markdown,
    Html,
    Trump,
    Screener,
    Geopolitics,
}

impl Kind {
    pub const ALL: &'static [Kind] = &[
        Kind::Watchlist,
        Kind::Chart,
        Kind::Details,
        Kind::NewsFeed,
        Kind::Portfolio,
        Kind::Notification,
        Kind::SmartMoney,
        Kind::AiChat,
        Kind::EconomicCalendar,
        Kind::Position,
        Kind::Execution,
        Kind::Markdown,
        Kind::Html,
        Kind::Trump,
        Kind::Screener,
        Kind::Geopolitics,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Kind::Watchlist => "Watchlist",
            Kind::Chart => "Chart",
            Kind::Details => "Details",
            Kind::NewsFeed => "NewsFeed",
            Kind::Portfolio => "Portfolio",
            Kind::Notification => "Notification",
            Kind::SmartMoney => "SmartMoney",
            Kind::AiChat => "AiChat",
            Kind::EconomicCalendar => "EconomicCalendar",
            Kind::Position => "Position",
            Kind::Execution => "Execution",
            Kind::Markdown => "Markdown",
            Kind::Html => "Html",
            Kind::Trump => "Trump",
            Kind::Screener => "Screener",
            Kind::Geopolitics => "Geopolitics",
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            Kind::NewsFeed => "News Feed",
            Kind::SmartMoney => "Smart Money",
            Kind::AiChat => "AI Chat",
            Kind::EconomicCalendar => "Economic Calendar",
            Kind::Markdown => "Markdown Test",
            Kind::Html => "HTML Test",
            Kind::Trump => "Trump Tracker",
            other => other.id(),
        }
    }

    pub fn from_id(id: &str) -> Option<Kind> {
        Self::ALL.iter().copied().find(|k| k.id() == id)
    }

    /// Singleton kinds may only have one instance live at a time. The toolbar
    /// toggles and the +Panel menu both consult this to avoid duplicates.
    pub fn is_singleton(self) -> bool {
        matches!(self, Kind::AiChat | Kind::Position | Kind::Execution)
    }
}

/// Global tracker for the most recently focused [`TabPanel`].
///
/// Why: gpui-component's `DockArea::add_panel` only takes a `DockPlacement` (Center/Left/Right/
/// Bottom), so it can't target a *specific* TabPanel. We track focus ourselves so the "+ Panel"
/// menu can drop new panels into whichever pane the user last clicked on.
#[derive(Default)]
pub struct LastFocusedTabPanel(pub Rc<RefCell<Option<WeakEntity<TabPanel>>>>);
impl Global for LastFocusedTabPanel {}

pub fn init(cx: &mut App) {
    cx.set_global(LastFocusedTabPanel::default());
    for kind in Kind::ALL {
        let kind = *kind;
        register_panel(cx, kind.id(), move |_dock_area, _state, _info, window, cx| {
            match kind {
                Kind::EconomicCalendar => Box::new(
                    cx.new(|cx| crate::economic_calendar::EconomicCalendarPanel::new(window, cx)),
                ),
                _ => Box::new(cx.new(|cx| ContentPanel::new(kind, window, cx))),
            }
        });
    }
}

pub fn build_kind(kind: Kind, window: &mut Window, cx: &mut App) -> Arc<dyn PanelView> {
    match kind {
        Kind::EconomicCalendar => Arc::new(
            cx.new(|cx| crate::economic_calendar::EconomicCalendarPanel::new(window, cx)),
        ),
        _ => Arc::new(cx.new(|cx| ContentPanel::new(kind, window, cx))),
    }
}

/// Per-chart-panel action: switch the chart's underlying asset. Dispatched
/// from the symbol-selector dropdown; the Chart panel handles it directly via
/// `.on_action` registered on its outer div, which receives the dispatch
/// because the popup menu sets its `action_context` to the panel's focus.
#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = terminal_demo, no_json)]
pub struct ChangeChartSymbol(pub SharedString);

pub struct ContentPanel {
    kind: Kind,
    focus_handle: FocusHandle,
    parent_tab_panel: Option<WeakEntity<TabPanel>>,
    chat_input: Option<Entity<InputState>>,
    /// Execution-panel inputs. Set only when `kind == Kind::Execution`.
    exec_inputs: Option<ExecutionInputs>,
    /// Chart-panel viewport state (symbol, candles, pan/zoom). Set only when
    /// `kind == Kind::Chart`.
    chart_state: Option<ChartState>,
}

#[derive(Clone)]
pub struct ExecutionInputs {
    pub symbol: Entity<InputState>,
    pub quantity: Entity<InputState>,
    pub limit: Entity<InputState>,
}

impl ContentPanel {
    pub fn new(kind: Kind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        // Only the AI chat panel has a real input; other kinds don't pay the InputState cost.
        // `auto_grow` wraps long prompts and grows the input up to 8 rows before
        // it starts internal scrolling (useful for "Ask AI" prefilled prompts).
        let chat_input = matches!(kind, Kind::AiChat).then(|| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .auto_grow(1, 8)
                    .placeholder("Ask anything…")
            })
        });
        let exec_inputs = matches!(kind, Kind::Execution).then(|| ExecutionInputs {
            symbol: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("AAPL")
                    .default_value("AAPL")
            }),
            quantity: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("100")
                    .default_value("100")
            }),
            limit: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("0.00")
                    .default_value("192.15")
            }),
        });
        let chart_state = matches!(kind, Kind::Chart).then(|| ChartState::new(CHART_SYMBOLS[0].0));
        Self {
            kind,
            focus_handle,
            parent_tab_panel: None,
            chat_input,
            exec_inputs,
            chart_state,
        }
    }

    fn on_change_chart_symbol(
        &mut self,
        action: &ChangeChartSymbol,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.chart_state.as_mut() else { return; };
        if state.symbol == action.0 {
            return;
        }
        *state = ChartState::new(action.0.as_ref());
        cx.notify();
    }

    pub fn parent_tab_panel(&self) -> Option<WeakEntity<TabPanel>> {
        self.parent_tab_panel.clone()
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The AI Chat input. `Some` only when `kind == Kind::AiChat`. Lets the
    /// workspace prefill the prompt for the "Ask AI" flow.
    pub fn chat_input(&self) -> Option<&Entity<InputState>> {
        self.chat_input.as_ref()
    }

    fn mark_focused(&self, cx: &mut App) {
        // Singleton panels (AI Chat, Position, Execution) are host-managed:
        // they live in pinned TabPanels and shouldn't become the "+ Panel"
        // drop target, so we don't record their parent here. Using `self.kind`
        // (rather than reading the parent TabPanel) is essential — this fires
        // from inside `TabPanel::set_active_ix`'s update closure, so reading
        // the parent's `is_pinned` would double-borrow the TabPanel.
        if self.kind.is_singleton() {
            return;
        }
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
}

impl EventEmitter<PanelEvent> for ContentPanel {}

impl Focusable for ContentPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for ContentPanel {
    fn panel_name(&self) -> &'static str {
        self.kind.id()
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        SharedString::from(self.kind.display())
    }

    fn on_added_to(
        &mut self,
        tab_panel: WeakEntity<TabPanel>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.parent_tab_panel = Some(tab_panel);
    }

    /// Clear the cached parent on detach so callers can tell the panel is no
    /// longer in the dock tree (e.g. user closed the tab). For drag-between
    /// TabPanels, on_added_to immediately re-sets the parent on the destination.
    fn on_removed(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.parent_tab_panel = None;
    }

    // Tab-changes also count as focus changes. on_focus_in handles body clicks; this handles
    // tab-strip clicks where the active tab swaps.
    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            self.mark_focused(cx);
        }
    }
}

impl Render for ContentPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let raw_body = match self.kind {
            Kind::Watchlist => render_watchlist(window, cx).into_any_element(),
            Kind::Chart => render_chart(
                self.chart_state.as_ref().expect("chart_state set for Chart"),
                self.focus_handle.clone(),
                window,
                cx,
            )
            .into_any_element(),
            Kind::Details => render_details(window, cx).into_any_element(),
            Kind::NewsFeed => render_news(window, cx).into_any_element(),
            Kind::Portfolio => render_portfolio(window, cx).into_any_element(),
            Kind::Notification => render_notifications(window, cx).into_any_element(),
            Kind::SmartMoney => render_smart_money(window, cx).into_any_element(),
            Kind::AiChat => render_ai_chat(
                self.chat_input.as_ref().expect("chat_input set for AiChat"),
                window,
                cx,
            )
            .into_any_element(),
            Kind::Position => render_position(window, cx).into_any_element(),
            Kind::Execution => render_execution(
                self.exec_inputs.as_ref().expect("exec_inputs set for Execution"),
                window,
                cx,
            )
            .into_any_element(),
            Kind::Markdown => render_markdown_test(window, cx).into_any_element(),
            Kind::Html => render_html_test(window, cx).into_any_element(),
            Kind::Trump => render_trump(window, cx).into_any_element(),
            Kind::Screener => render_screener(window, cx).into_any_element(),
            Kind::Geopolitics => render_geopolitics(window, cx).into_any_element(),
            Kind::EconomicCalendar => unreachable!(
                "EconomicCalendar is handled by EconomicCalendarPanel, not ContentPanel"
            ),
        };
        // AiChat manages its own internal scroll region (so its input bar stays pinned at
        // the bottom). Chart fills the available space so the canvas can flex with the
        // panel (no vertical scroll). Every other kind gets a single outer scroll wrapper
        // so long lists don't get clipped when the panel shrinks.
        let body = if matches!(self.kind, Kind::AiChat | Kind::Chart) {
            raw_body
        } else {
            div()
                .id(SharedString::from(format!("scroll-{}", self.kind.id())))
                .size_full()
                .overflow_y_scroll()
                .child(raw_body)
                .into_any_element()
        };
        // Border is always 2px wide so toggling focus doesn't shift content; only the color
        // changes between transparent (unfocused) and theme.ring (focused).
        let border_color = if self.is_focused(cx) {
            cx.theme().ring
        } else {
            gpui::transparent_black()
        };
        // Click-based focus tracking (NOT track_focus / on_focus_in) — gpui's web focus
        // mechanism uses a hidden input element which makes mobile browsers pop the soft
        // keyboard on every tap. Mouse-down works the same on touch and doesn't claim
        // text-input focus.
        div()
            .id(SharedString::from(format!("panel-body-{}", self.kind.id())))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev, _window, cx| this.mark_focused(cx)),
            )
            // `track_focus` is the focus channel that the chart's symbol-selector
            // popup uses to dispatch `ChangeChartSymbol` back into this panel.
            // We scope it to Chart only because gpui's web focus mechanism
            // creates a hidden <input>, and broadly applied focus tracking pops
            // the soft keyboard on mobile (see CLAUDE.md).
            .when(matches!(self.kind, Kind::Chart), |this| {
                this.track_focus(&self.focus_handle)
                    .on_action(cx.listener(Self::on_change_chart_symbol))
            })
            .size_full()
            .border_2()
            .border_color(border_color)
            .child(body)
    }
}

// ============================================================================
// Watchlist
// ============================================================================

#[derive(Clone)]
struct Ticker {
    symbol: &'static str,
    name: &'static str,
    last: f64,
    change_pct: f64,
}

const WATCHLIST: &[Ticker] = &[
    Ticker { symbol: "AAPL",  name: "Apple Inc.",         last: 185.32, change_pct:  1.23 },
    Ticker { symbol: "MSFT",  name: "Microsoft Corp.",    last: 378.45, change_pct: -0.45 },
    Ticker { symbol: "NVDA",  name: "NVIDIA Corp.",       last: 875.21, change_pct:  3.87 },
    Ticker { symbol: "GOOGL", name: "Alphabet Inc.",      last: 142.18, change_pct:  0.62 },
    Ticker { symbol: "TSLA",  name: "Tesla, Inc.",        last: 248.50, change_pct: -1.89 },
    Ticker { symbol: "META",  name: "Meta Platforms",     last: 502.34, change_pct:  2.14 },
    Ticker { symbol: "AMZN",  name: "Amazon.com",         last: 178.92, change_pct:  0.78 },
    Ticker { symbol: "BRK.B", name: "Berkshire Hathaway", last: 412.65, change_pct: -0.12 },
];

fn render_watchlist(_window: &mut Window, cx: &mut Context<ContentPanel>) -> impl IntoElement {
    let theme = cx.theme();
    let bullish = theme.chart_bullish;
    let bearish = theme.chart_bearish;
    let muted = theme.muted_foreground;
    let border = theme.border;

    v_flex()
        .w_full()
        .p_2()
        .gap_1()
        .child(
            h_flex()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(muted)
                .border_b_1()
                .border_color(border)
                .child(div().w_20().child("Symbol"))
                .child(div().flex_1().child("Name"))
                .child(div().w_24().text_right().child("Last"))
                .child(div().w_20().text_right().child("Chg %")),
        )
        .children(WATCHLIST.iter().map(|t| {
            let color = if t.change_pct >= 0.0 { bullish } else { bearish };
            h_flex()
                .px_2()
                .py_1()
                .text_sm()
                .child(div().w_20().font_semibold().child(t.symbol))
                .child(div().flex_1().text_color(muted).child(t.name))
                .child(div().w_24().text_right().child(format!("{:.2}", t.last)))
                .child(
                    div()
                        .w_20()
                        .text_right()
                        .text_color(color)
                        .child(format!("{:+.2}%", t.change_pct)),
                )
        }))
}

// ============================================================================
// Chart
// ============================================================================

#[derive(Clone)]
struct Candle {
    date: SharedString,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
}

const CHART_SYMBOLS: &[(&str, &str, &str)] = &[
    ("AAPL",  "Apple Inc.",         "NASDAQ"),
    ("MSFT",  "Microsoft Corp.",    "NASDAQ"),
    ("NVDA",  "NVIDIA Corp.",       "NASDAQ"),
    ("GOOGL", "Alphabet Inc.",      "NASDAQ"),
    ("TSLA",  "Tesla, Inc.",        "NASDAQ"),
    ("META",  "Meta Platforms",     "NASDAQ"),
    ("AMZN",  "Amazon.com",         "NASDAQ"),
    ("BRK.B", "Berkshire Hathaway", "NYSE"),
];

/// Real 1h OHLC bars fetched by `scripts/fetch_chart_data.py`. Embedded so
/// the WASM bundle is self-contained — no network call at runtime.
const CHART_DATA_RAW: &str = include_str!("../assets/chart_data.json");

const CHART_DEFAULT_VIEW: f32 = 60.0;
const CHART_MIN_VIEW: f32 = 8.0;
/// Fallback candle count used only when a symbol isn't present in the
/// embedded JSON (e.g. fetch hasn't been run, or the symbol is new).
const CHART_FALLBACK_CANDLES: usize = 240;

#[derive(Deserialize)]
struct RawCandle {
    t: String,
    o: f64,
    h: f64,
    l: f64,
    c: f64,
}

#[derive(Deserialize)]
struct RawSeries {
    bars: Vec<RawCandle>,
}

/// Parse the embedded JSON once into a symbol-keyed candle map. Lazy so the
/// ~200KB JSON parse cost is paid on first chart open, not at startup.
fn chart_data() -> &'static HashMap<String, Vec<Candle>> {
    static DATA: OnceLock<HashMap<String, Vec<Candle>>> = OnceLock::new();
    DATA.get_or_init(|| {
        let raw: HashMap<String, RawSeries> = serde_json::from_str(CHART_DATA_RAW)
            .expect("embedded chart_data.json is malformed");
        raw.into_iter()
            .map(|(symbol, series)| {
                let candles = series
                    .bars
                    .into_iter()
                    .map(|b| Candle {
                        date: SharedString::from(b.t),
                        open: b.o,
                        high: b.h,
                        low: b.l,
                        close: b.c,
                    })
                    .collect();
                (symbol, candles)
            })
            .collect()
    })
}

pub struct ChartState {
    symbol: SharedString,
    name: SharedString,
    exchange: SharedString,
    candles: Vec<Candle>,
    /// Fractional left-edge index of the visible window. Fractional so pan
    /// stays smooth at sub-candle granularity even though the chart paints
    /// integer-indexed bars.
    view_start: f32,
    /// Number of candles visible in the viewport (fractional for the same
    /// reason as `view_start`).
    view_size: f32,
    /// `(mouse_down_position, view_start_at_down)` — set on left-mouse-down,
    /// cleared on up. While present, mouse-move pans the view.
    drag_anchor: Option<(Point<Pixels>, f32)>,
    /// Last painted bounds of the chart canvas. Captured via `on_prepaint`
    /// and consumed by drag/wheel handlers to convert pixel deltas into
    /// candle-space deltas.
    bounds: Option<Bounds<Pixels>>,
    /// When true the price (y) axis auto-fits to the visible candles each
    /// render. Flipping to false locks the axis to (`y_min`, `y_max`) so
    /// users can drag/wheel the right edge to scale price independently.
    /// Restored to true via double-click on the right axis.
    y_auto: bool,
    /// Locked price-axis range. Only consulted when `y_auto` is false.
    y_min: f64,
    y_max: f64,
    /// Drag anchor for vertical-only manipulation on the right axis:
    /// `(mouse_down_position, y_min_at_down, y_max_at_down)`.
    y_drag_anchor: Option<(Point<Pixels>, f64, f64)>,
    /// Drag anchor for horizontal-only zoom on the bottom axis:
    /// `(mouse_down_position, view_size_at_down)`.
    x_axis_drag_anchor: Option<(Point<Pixels>, f32)>,
}

impl ChartState {
    fn new(symbol: &str) -> Self {
        let (sym, name, exchange) = CHART_SYMBOLS
            .iter()
            .copied()
            .find(|(s, _, _)| *s == symbol)
            .unwrap_or(CHART_SYMBOLS[0]);
        // Prefer the real fetched 1h bars; only synthesize when the JSON
        // doesn't carry this symbol (keeps the demo working if someone adds
        // a new ticker before re-running the fetch script).
        let candles = chart_data()
            .get(sym)
            .cloned()
            .unwrap_or_else(|| generate_candles(sym));
        let total = candles.len() as f32;
        Self {
            symbol: SharedString::from(sym),
            name: SharedString::from(name),
            exchange: SharedString::from(exchange),
            candles,
            view_start: (total - CHART_DEFAULT_VIEW).max(0.0),
            view_size: CHART_DEFAULT_VIEW.min(total),
            drag_anchor: None,
            bounds: None,
            y_auto: true,
            y_min: 0.0,
            y_max: 0.0,
            y_drag_anchor: None,
            x_axis_drag_anchor: None,
        }
    }

    fn clamp(&mut self) {
        let total = self.candles.len() as f32;
        self.view_size = self.view_size.clamp(CHART_MIN_VIEW.min(total), total);
        let max_start = (total - self.view_size).max(0.0);
        self.view_start = self.view_start.clamp(0.0, max_start);
    }

    fn visible(&self) -> Vec<Candle> {
        let start = self.view_start.max(0.0).floor() as usize;
        let take = self.view_size.ceil() as usize;
        let end = (start + take).min(self.candles.len());
        self.candles[start..end].to_vec()
    }

    /// Auto-fit price range from the visible candles. Returned `(min, max)`
    /// with a small padding so candles don't touch the chart edges.
    fn auto_y_range(&self) -> (f64, f64) {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for c in self.visible() {
            lo = lo.min(c.low);
            hi = hi.max(c.high);
        }
        if !lo.is_finite() || !hi.is_finite() || hi <= lo {
            return (0.0, 1.0);
        }
        let pad = (hi - lo) * 0.05;
        (lo - pad, hi + pad)
    }

    /// Lock the price axis to the current auto-fit range. Called the moment
    /// the user starts manipulating the right axis so subsequent drag/wheel
    /// moves work from a stable baseline instead of fighting auto-fit.
    fn freeze_y_if_auto(&mut self) {
        if self.y_auto {
            let (lo, hi) = self.auto_y_range();
            self.y_min = lo;
            self.y_max = hi;
            self.y_auto = false;
        }
    }

    fn reset_y_auto(&mut self) {
        self.y_auto = true;
        self.y_drag_anchor = None;
    }

    /// Reset the time axis to the default trailing window (most recent
    /// `CHART_DEFAULT_VIEW` candles). Used by double-click on the bottom axis.
    fn reset_x(&mut self) {
        let total = self.candles.len() as f32;
        self.view_size = CHART_DEFAULT_VIEW.min(total);
        self.view_start = (total - self.view_size).max(0.0);
        self.x_axis_drag_anchor = None;
    }
}

/// Deterministic per-symbol synthetic candle generator. Used as a fallback
/// when a symbol isn't present in the embedded fetched dataset. FNV-hashes
/// the symbol into a seed, then walks bars with sin/cos drift so each symbol
/// gets a distinct but stable price series.
fn generate_candles(symbol: &str) -> Vec<Candle> {
    let mut seed: u64 = 0xcbf29ce484222325;
    for b in symbol.bytes() {
        seed ^= b as u64;
        seed = seed.wrapping_mul(0x100000001b3);
    }
    let base = 50.0 + (seed % 800) as f64;
    let trend_sign = if (seed >> 17) & 1 == 0 { 1.0 } else { -1.0 };
    let trend = trend_sign * (((seed >> 9) % 7) as f64 * 0.05 + 0.05);

    let mut close = base;
    let mut v = Vec::with_capacity(CHART_FALLBACK_CANDLES);
    for i in 0..CHART_FALLBACK_CANDLES {
        let t = i as f64;
        let s = ((seed.wrapping_add(i as u64 * 2654435761)) & 0xffff) as f64 / 65535.0 - 0.5;
        let drift = (t * 0.13).sin() * (base * 0.020)
            + (t * 0.041).cos() * (base * 0.010)
            + s * (base * 0.015)
            + trend;
        let open = close + (t * 1.07 + (seed % 13) as f64).cos() * (base * 0.005);
        close = (open + drift).max(base * 0.30);
        let amp = (base * 0.005) + ((t * 0.21).sin().abs() * base * 0.012);
        let high = open.max(close) + amp;
        let low = (open.min(close) - amp).max(base * 0.20);
        v.push(Candle {
            date: SharedString::from(format!("D{:03}", i + 1)),
            open,
            high,
            low,
            close,
        });
    }
    v
}

fn render_chart(
    state: &ChartState,
    focus: FocusHandle,
    _window: &mut Window,
    cx: &mut Context<ContentPanel>,
) -> impl IntoElement {
    let theme = cx.theme();
    let visible = state.visible();
    let last_close = visible.last().map(|c| c.close).unwrap_or(0.0);
    let first_close = visible.first().map(|c| c.close).unwrap_or(last_close);
    let price_color = if last_close >= first_close {
        theme.chart_bullish
    } else {
        theme.chart_bearish
    };

    // Symbol-selector dropdown. Setting `action_context` to the panel's focus
    // handle ensures `ChangeChartSymbol` dispatches up through *this* panel
    // (not whichever element happened to have focus when the menu opened),
    // so multiple Chart panels stay independent.
    let dropdown_focus = focus.clone();
    let symbol_btn = Button::new("chart-symbol-select")
        .label(state.symbol.clone())
        .small()
        .ghost()
        .dropdown_menu(move |menu, _, _| {
            let mut menu = menu.action_context(dropdown_focus.clone());
            for (sym, name, _ex) in CHART_SYMBOLS {
                menu = menu.menu(
                    SharedString::from(format!("{}  ·  {}", sym, name)),
                    Box::new(ChangeChartSymbol(SharedString::from(*sym))),
                );
            }
            menu
        });

    let candles_for_main = visible;
    let entity = cx.entity();
    let dragging = state.drag_anchor.is_some();
    // Auto-thin x-axis labels: aim for ~one label per `LABEL_PIXEL_BUDGET`
    // pixels of canvas width so timestamps don't overlap. Falls back to a
    // density derived from view_size on the very first frame, before bounds
    // have been captured by `on_prepaint`.
    let tick_margin = {
        const LABEL_PIXEL_BUDGET: f32 = 70.0;
        let approx_width = state
            .bounds
            .map(|b| b.size.width.as_f32())
            .unwrap_or(600.0);
        let max_labels = (approx_width / LABEL_PIXEL_BUDGET).floor().max(2.0) as usize;
        ((state.view_size as usize).max(1) / max_labels).max(1)
    };
    // Locked y-axis range when the user has manipulated the right axis;
    // None means the chart auto-fits price each render.
    let y_domain = (!state.y_auto).then(|| (state.y_min, state.y_max));

    // Right (price) axis interaction zone — overlays the chart's reserved
    // y-label gutter. Vertical drag scales the locked y range; wheel zooms
    // it; double-click re-enables auto-fit.
    let y_axis_gap = nice_y_axis_gap();
    let right_axis = div()
        .id("chart-right-axis")
        .absolute()
        .right_0()
        .top_0()
        .bottom(gpui::px(AXIS_GAP))
        .w(gpui::px(y_axis_gap))
        .cursor_ns_resize()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, ev: &MouseDownEvent, _w, cx| {
                cx.stop_propagation();
                let Some(state) = this.chart_state.as_mut() else { return; };
                if ev.click_count >= 2 {
                    state.reset_y_auto();
                    cx.notify();
                    return;
                }
                state.freeze_y_if_auto();
                state.y_drag_anchor = Some((ev.position, state.y_min, state.y_max));
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _w, cx| {
            let Some(state) = this.chart_state.as_mut() else { return; };
            if !ev.dragging() {
                if state.y_drag_anchor.take().is_some() {
                    cx.notify();
                }
                return;
            }
            let Some((start_pos, start_lo, start_hi)) = state.y_drag_anchor else { return; };
            let Some(bounds) = state.bounds else { return; };
            let h = bounds.size.height.as_f32();
            if h <= 0.0 {
                return;
            }
            let dy = ev.position.y.as_f32() - start_pos.y.as_f32();
            // Drag down → range expands (zoom out); drag up → contracts.
            let factor = (dy / h).exp() as f64;
            let center = (start_lo + start_hi) / 2.0;
            state.y_min = center - (center - start_lo) * factor;
            state.y_max = center + (start_hi - center) * factor;
            cx.notify();
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _ev, _w, cx| {
                let Some(state) = this.chart_state.as_mut() else { return; };
                if state.y_drag_anchor.take().is_some() {
                    cx.notify();
                }
            }),
        )
        .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, w, cx| {
            cx.stop_propagation();
            let Some(state) = this.chart_state.as_mut() else { return; };
            let delta_y = ev.delta.pixel_delta(w.line_height()).y.as_f32();
            if delta_y == 0.0 {
                return;
            }
            state.freeze_y_if_auto();
            let factor = (-delta_y / 120.0).exp() as f64;
            let center = (state.y_min + state.y_max) / 2.0;
            state.y_min = center - (center - state.y_min) * factor;
            state.y_max = center + (state.y_max - center) * factor;
            cx.notify();
        }));

    // Bottom (time) axis interaction zone. Horizontal drag scales view_size
    // around its centre; wheel zooms; double-click resets to the trailing
    // default window.
    let bottom_axis = div()
        .id("chart-bottom-axis")
        .absolute()
        .left_0()
        .bottom_0()
        .right(gpui::px(y_axis_gap))
        .h(gpui::px(AXIS_GAP))
        .cursor_ew_resize()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, ev: &MouseDownEvent, _w, cx| {
                cx.stop_propagation();
                let Some(state) = this.chart_state.as_mut() else { return; };
                if ev.click_count >= 2 {
                    state.reset_x();
                    cx.notify();
                    return;
                }
                state.x_axis_drag_anchor = Some((ev.position, state.view_size));
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _w, cx| {
            let Some(state) = this.chart_state.as_mut() else { return; };
            if !ev.dragging() {
                if state.x_axis_drag_anchor.take().is_some() {
                    cx.notify();
                }
                return;
            }
            let Some((start_pos, start_size)) = state.x_axis_drag_anchor else { return; };
            let Some(bounds) = state.bounds else { return; };
            let w = bounds.size.width.as_f32();
            if w <= 0.0 {
                return;
            }
            let dx = ev.position.x.as_f32() - start_pos.x.as_f32();
            // Drag right → view widens (more candles), drag left → narrows.
            let factor = (dx / w).exp();
            let center = state.view_start + state.view_size / 2.0;
            state.view_size = (start_size * factor).max(CHART_MIN_VIEW);
            state.view_start = center - state.view_size / 2.0;
            state.clamp();
            cx.notify();
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _ev, _w, cx| {
                let Some(state) = this.chart_state.as_mut() else { return; };
                if state.x_axis_drag_anchor.take().is_some() {
                    cx.notify();
                }
            }),
        )
        .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, w, cx| {
            cx.stop_propagation();
            let Some(state) = this.chart_state.as_mut() else { return; };
            let delta_y = ev.delta.pixel_delta(w.line_height()).y.as_f32();
            if delta_y == 0.0 {
                return;
            }
            let factor = (-delta_y / 120.0).exp();
            let center = state.view_start + state.view_size / 2.0;
            state.view_size *= factor;
            state.view_start = center - state.view_size / 2.0;
            state.clamp();
            cx.notify();
        }));

    let canvas = div()
        .id("chart-canvas")
        .relative()
        .flex_1()
        .min_h_0()
        .w_full()
        .map(|this| if dragging { this.cursor_grabbing() } else { this.cursor_grab() })
        .on_prepaint({
            let entity = entity.clone();
            move |bounds, _, cx| {
                entity.update(cx, |this, _| {
                    if let Some(state) = this.chart_state.as_mut() {
                        state.bounds = Some(bounds);
                    }
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, ev: &MouseDownEvent, _w, cx| {
                let Some(state) = this.chart_state.as_mut() else { return; };
                state.drag_anchor = Some((ev.position, state.view_start));
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _w, cx| {
            let Some(state) = this.chart_state.as_mut() else { return; };
            if !ev.dragging() {
                if state.drag_anchor.take().is_some() {
                    cx.notify();
                }
                return;
            }
            let Some((start_pos, start_view)) = state.drag_anchor else { return; };
            let Some(bounds) = state.bounds else { return; };
            let width = bounds.size.width.as_f32();
            if width <= 0.0 {
                return;
            }
            let dx = ev.position.x.as_f32() - start_pos.x.as_f32();
            let candles_per_px = state.view_size / width;
            state.view_start = start_view - dx * candles_per_px;
            state.clamp();
            cx.notify();
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _ev, _w, cx| {
                let Some(state) = this.chart_state.as_mut() else { return; };
                if state.drag_anchor.take().is_some() {
                    cx.notify();
                }
            }),
        )
        .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, w, cx| {
            let Some(state) = this.chart_state.as_mut() else { return; };
            let delta_y = ev.delta.pixel_delta(w.line_height()).y.as_f32();
            if delta_y == 0.0 {
                return;
            }
            // Wheel-up (positive delta_y) zooms IN (smaller view_size); wheel-down zooms out.
            let factor = (-delta_y / 120.0).exp();
            // Anchor the zoom around the cursor's x within the chart, like TradingView.
            let anchor_offset = state
                .bounds
                .map(|b| {
                    let w = b.size.width.as_f32();
                    if w > 0.0 {
                        ((ev.position.x.as_f32() - b.origin.x.as_f32()) / w).clamp(0.0, 1.0)
                    } else {
                        0.5
                    }
                })
                .unwrap_or(0.5);
            let world_anchor = state.view_start + state.view_size * anchor_offset;
            state.view_size *= factor;
            state.view_start = world_anchor - state.view_size * anchor_offset;
            state.clamp();
            cx.notify();
        }))
        .child(
            CandlestickChart::new(candles_for_main)
                .x(|c: &Candle| c.date.clone())
                .open(|c: &Candle| c.open)
                .close(|c: &Candle| c.close)
                .high(|c: &Candle| c.high)
                .low(|c: &Candle| c.low)
                .tick_margin(tick_margin)
                .y_axis(true)
                .y_domain(y_domain),
        )
        // Axis zones go AFTER the chart so they sit on top in z-order and
        // get hit-tested first — their handlers `cx.stop_propagation()` to
        // keep mouse-down from also arming the canvas's pan drag.
        .child(right_axis)
        .child(bottom_axis);

    v_flex()
        .size_full()
        .p_3()
        .gap_2()
        .child(
            h_flex()
                .gap_3()
                .items_center()
                .child(symbol_btn)
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .text_sm()
                        .child(format!("{} · {}", state.name, state.exchange)),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_color(price_color)
                        .font_semibold()
                        .child(format!("${:.2}", last_close)),
                ),
        )
        .child(canvas)
}

// ============================================================================
// Markdown Test
// ============================================================================

const MARKDOWN_DUMMY: &str = r#"# Markdown Rendering Test

A sandbox panel exercising **gpui-component**'s built-in `text::markdown()`
helper. No browser involved — this is rendered natively by the same widget
that powers the AI Chat reply view.

---

## Inline formatting

Regular text mixed with **bold**, *italic*, ***bold-italic***, `inline code`,
~~strikethrough~~, and a [link to example.com](https://example.com).

> Block quotes wrap nicely and pick up the muted-foreground colour from the
> active theme. They're useful for callouts and pull-quotes inside longer
> explanations.

## Lists

Unordered:

- First bullet
- Second bullet with **emphasis**
  - Nested item alpha
  - Nested item beta
- Third bullet with `code`

Ordered:

1. Set up the dock layout
2. Add the panel to the registry
3. Wire up the render dispatch
4. Profit

Task list:

- [x] Render headings
- [x] Render lists
- [ ] Wire up table-of-contents
- [ ] Persist scroll position

## Code

Inline `let x = 42;` and a fenced block:

```rust
fn fibonacci(n: u32) -> u64 {
    let (mut a, mut b) = (0u64, 1u64);
    for _ in 0..n {
        let next = a + b;
        a = b;
        b = next;
    }
    a
}
```

```python
def greet(name: str) -> str:
    return f"hello, {name}"
```

## Tables

| Symbol | Last    | Δ%      | Sector       |
|--------|--------:|--------:|--------------|
| AAPL   | 185.32  | +1.23%  | Technology   |
| MSFT   | 378.45  | -0.45%  | Technology   |
| NVDA   | 875.21  | +3.87%  | Technology   |
| TSLA   | 248.50  | -1.89%  | Auto         |
| BRK.B  | 412.65  | -0.12%  | Financials   |

## Headings cascade

### Level 3
#### Level 4
##### Level 5
###### Level 6

That's the lot — scroll up to confirm spacing and rhythm look right.
"#;

fn render_markdown_test(
    _window: &mut Window,
    _cx: &mut Context<ContentPanel>,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .p_4()
        .gap_2()
        .child(markdown(MARKDOWN_DUMMY))
}

// ============================================================================
// HTML Test
// ============================================================================

const HTML_DUMMY: &str = r#"<h1>HTML Rendering Test</h1>
<p>
  A sandbox panel exercising <strong>gpui-component</strong>'s
  <code>text::html()</code> helper. The library parses HTML with
  <em>html5ever</em> and renders the supported subset directly into GPUI —
  no browser engine, no iframe, no DOM.
</p>

<hr/>

<h2>Inline formatting</h2>
<p>
  Regular text mixed with <b>bold</b>, <i>italic</i>, <u>underline</u>,
  <code>inline code</code>, <s>strikethrough</s>, and a
  <a href="https://example.com">link to example.com</a>.
</p>

<blockquote>
  Block quotes wrap and inherit a muted foreground colour from the active
  theme. The HTML parser also accepts nested formatting, so
  <em>emphasis inside quotes</em> works.
</blockquote>

<h2>Lists</h2>
<ul>
  <li>Unordered alpha</li>
  <li>Unordered beta with <strong>emphasis</strong></li>
  <li>
    Nested:
    <ul>
      <li>inner one</li>
      <li>inner two</li>
    </ul>
  </li>
</ul>

<ol>
  <li>Ordered first</li>
  <li>Ordered second</li>
  <li>Ordered third</li>
</ol>

<h2>Code</h2>
<pre><code>fn add(a: i32, b: i32) -&gt; i32 {
    a + b
}
</code></pre>

<h2>Table</h2>
<table>
  <thead>
    <tr><th>Ticker</th><th>Last</th><th>Δ%</th><th>Sector</th></tr>
  </thead>
  <tbody>
    <tr><td>AAPL</td><td>185.32</td><td>+1.23%</td><td>Technology</td></tr>
    <tr><td>MSFT</td><td>378.45</td><td>-0.45%</td><td>Technology</td></tr>
    <tr><td>NVDA</td><td>875.21</td><td>+3.87%</td><td>Technology</td></tr>
    <tr><td>TSLA</td><td>248.50</td><td>-1.89%</td><td>Auto</td></tr>
    <tr><td>BRK.B</td><td>412.65</td><td>-0.12%</td><td>Financials</td></tr>
  </tbody>
</table>

<h2>Headings cascade</h2>
<h3>Level 3</h3>
<h4>Level 4</h4>
<h5>Level 5</h5>
<h6>Level 6</h6>

<p>That's the lot — anything broken above is a parser limitation, not a
content issue.</p>
"#;

fn render_html_test(
    _window: &mut Window,
    _cx: &mut Context<ContentPanel>,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .p_4()
        .gap_2()
        .child(html(HTML_DUMMY))
}

// ============================================================================
// Trump Tracker
// ============================================================================

#[derive(Clone, Copy)]
enum TrumpChannel {
    TruthSocial,
    Speech,
    Press,
    Rally,
    Interview,
}

struct TrumpPost {
    when: &'static str,
    channel: TrumpChannel,
    location: &'static str,
    headline: &'static str,
    excerpt: &'static str,
    impact_tags: &'static [&'static str],
    /// Rough market lean inferred from the post — bullish/bearish/neutral risk.
    sentiment: Option<bool>,
    engagement: &'static str,
}

const TRUMP_POSTS: &[TrumpPost] = &[
    TrumpPost {
        when: "10:42",
        channel: TrumpChannel::TruthSocial,
        location: "@realDonaldTrump",
        headline: "“60% TARIFFS on Chinese EVs starting Day One. American jobs come first!”",
        excerpt: "Truth Social post promising sweeping auto tariffs if re-elected, naming BYD, Geely, and NIO as targets. Calls Detroit “a disaster waiting to be saved.”",
        impact_tags: &["TSLA", "F", "GM", "CHINA-A", "AUTO"],
        sentiment: Some(false),
        engagement: "82.4k reposts · 412k likes",
    },
    TrumpPost {
        when: "09:18",
        channel: TrumpChannel::Rally,
        location: "Erie, PA",
        headline: "Pledges to “end the EV mandate on day one” at Pennsylvania rally",
        excerpt: "60-minute speech focusing on energy policy. Promises to reopen leased federal lands for drilling and to “drill, baby, drill — bigger than ever.”",
        impact_tags: &["XOM", "CVX", "OIL", "TSLA", "RIVN"],
        sentiment: Some(true),
        engagement: "Live · 28k stream peak",
    },
    TrumpPost {
        when: "08:55",
        channel: TrumpChannel::TruthSocial,
        location: "@realDonaldTrump",
        headline: "“Powell is too late, AGAIN. Cut rates NOW or markets crash.”",
        excerpt: "Direct attack on Fed Chair ahead of FOMC. Claims the central bank is “politically captured” and demands an emergency 50bp cut.",
        impact_tags: &["FED", "DXY", "TLT", "SPX"],
        sentiment: None,
        engagement: "44.1k reposts · 198k likes",
    },
    TrumpPost {
        when: "Yesterday 22:10",
        channel: TrumpChannel::Interview,
        location: "Fox Business",
        headline: "Floats replacing income tax with universal tariff regime",
        excerpt: "In a 20-min interview, suggests a 10% across-the-board tariff could “fund the entire government.” Economists immediately push back; futures wobble.",
        impact_tags: &["WMT", "TGT", "AMZN", "USD"],
        sentiment: Some(false),
        engagement: "1.2M views · trending #1",
    },
    TrumpPost {
        when: "Yesterday 18:45",
        channel: TrumpChannel::TruthSocial,
        location: "@realDonaldTrump",
        headline: "“Bitcoin will be made in the USA. The CCP cannot have it.”",
        excerpt: "Endorses domestic mining and proposes a strategic Bitcoin reserve modeled on the SPR. Crypto markets rip 4% on the post.",
        impact_tags: &["BTC", "MARA", "RIOT", "COIN"],
        sentiment: Some(true),
        engagement: "118k reposts · 540k likes",
    },
    TrumpPost {
        when: "Yesterday 15:02",
        channel: TrumpChannel::Press,
        location: "Mar-a-Lago presser",
        headline: "Calls NATO members “delinquent” — threatens conditional defense",
        excerpt: "Says U.S. would only defend allies that meet 2% spending. European defense names spike on expectation of forced rearmament.",
        impact_tags: &["LMT", "RTX", "EU-DEF", "EUR"],
        sentiment: Some(true),
        engagement: "Pool feed · 47 outlets",
    },
    TrumpPost {
        when: "2d ago",
        channel: TrumpChannel::Speech,
        location: "CPAC keynote",
        headline: "“Day-one drilling permits — Alaska, Gulf, ANWR all reopen.”",
        excerpt: "Outlines an executive-order package to fast-track LNG export approvals and unwind the current pause. Energy ETFs gap up at the open.",
        impact_tags: &["XLE", "LNG", "OXY", "TPL"],
        sentiment: Some(true),
        engagement: "9k attendees · standing O",
    },
    TrumpPost {
        when: "2d ago",
        channel: TrumpChannel::TruthSocial,
        location: "@realDonaldTrump",
        headline: "“Pharma companies RIPPING OFF Americans. Prices coming down.”",
        excerpt: "Threatens executive action on drug pricing if Congress fails to deliver. Pharma sector sells off pre-market on policy-risk premium.",
        impact_tags: &["LLY", "PFE", "MRK", "XBI"],
        sentiment: Some(false),
        engagement: "31.7k reposts · 142k likes",
    },
];

fn render_trump(_window: &mut Window, cx: &mut Context<ContentPanel>) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let border = theme.border;
    let bullish = theme.chart_bullish;
    let bearish = theme.chart_bearish;
    let card_bg = theme.muted;

    let header = h_flex()
        .px_3()
        .py_2()
        .gap_2()
        .items_center()
        .border_b_1()
        .border_color(border)
        .child(div().size_2().rounded_full().bg(theme.chart_bearish))
        .child(div().text_sm().font_semibold().child("Trump Tracker"))
        .child(div().flex_1())
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child("Truth Social · speeches · pressers · interviews"),
        );

    let posts = v_flex()
        .w_full()
        .p_2()
        .gap_2()
        .children(TRUMP_POSTS.iter().enumerate().map(|(idx, p)| {
            let (channel_label, channel_color) = match p.channel {
                TrumpChannel::TruthSocial => ("TRUTH",  theme.chart_3),
                TrumpChannel::Speech      => ("SPEECH", theme.chart_5),
                TrumpChannel::Press       => ("PRESS",  theme.chart_4),
                TrumpChannel::Rally       => ("RALLY",  theme.chart_2),
                TrumpChannel::Interview   => ("INTV",   theme.chart_1),
            };
            let (sentiment_label, sentiment_color) = match p.sentiment {
                Some(true) => ("RISK-ON", bullish),
                Some(false) => ("RISK-OFF", bearish),
                None => ("MIXED", muted),
            };
            v_flex()
                .id(SharedString::from(format!("trump-row-{idx}")))
                .px_3()
                .py_2()
                .gap_2()
                .rounded(gpui::px(6.))
                .bg(card_bg)
                .border_1()
                .border_color(border)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .text_xs()
                        .child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded(gpui::px(3.))
                                .bg(channel_color)
                                .text_color(theme.background)
                                .font_semibold()
                                .child(channel_label),
                        )
                        .child(div().text_color(muted).child(p.location))
                        .child(div().flex_1())
                        .child(
                            div()
                                .font_semibold()
                                .text_color(sentiment_color)
                                .child(sentiment_label),
                        )
                        .child(div().text_color(muted).child(p.when)),
                )
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(theme.foreground)
                        .child(p.headline),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(p.excerpt),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .flex_wrap()
                        .children(p.impact_tags.iter().map(|t| {
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded(gpui::px(3.))
                                .bg(theme.accent)
                                .text_color(theme.accent_foreground)
                                .text_xs()
                                .child(*t)
                        })),
                )
                .child(
                    h_flex()
                        .items_center()
                        .text_xs()
                        .text_color(muted)
                        .child(div().flex_1().child(p.engagement)),
                )
        }));

    v_flex().w_full().child(header).child(posts)
}

// ============================================================================
// Screener
// ============================================================================

#[derive(Clone, Copy)]
enum ScreenerSignal {
    Breakout,
    Oversold,
    Squeeze,
    Reversal,
    Momentum,
}

struct ScreenerRow {
    symbol: &'static str,
    sector: &'static str,
    last: f64,
    change_pct: f64,
    rel_vol: f64,
    market_cap: &'static str,
    rsi: f64,
    signal: ScreenerSignal,
}

const SCREENER_ROWS: &[ScreenerRow] = &[
    ScreenerRow { symbol: "NVDA", sector: "Semiconductors", last: 875.21, change_pct: 3.87,  rel_vol: 2.4, market_cap: "$2.16T",  rsi: 68.4, signal: ScreenerSignal::Breakout },
    ScreenerRow { symbol: "META", sector: "Communications", last: 502.34, change_pct: 2.14,  rel_vol: 1.8, market_cap: "$1.28T",  rsi: 62.1, signal: ScreenerSignal::Momentum },
    ScreenerRow { symbol: "AAPL", sector: "Technology",     last: 185.32, change_pct: 1.23,  rel_vol: 1.1, market_cap: "$2.85T",  rsi: 54.7, signal: ScreenerSignal::Squeeze },
    ScreenerRow { symbol: "AMD",  sector: "Semiconductors", last: 162.85, change_pct: -2.41, rel_vol: 1.6, market_cap: "$263B",   rsi: 38.2, signal: ScreenerSignal::Oversold },
    ScreenerRow { symbol: "MU",   sector: "Semiconductors", last: 119.40, change_pct: 4.62,  rel_vol: 3.1, market_cap: "$132B",   rsi: 71.5, signal: ScreenerSignal::Breakout },
    ScreenerRow { symbol: "PLTR", sector: "Software",       last: 24.18,  change_pct: 5.94,  rel_vol: 2.9, market_cap: "$54B",    rsi: 74.8, signal: ScreenerSignal::Momentum },
    ScreenerRow { symbol: "CRWD", sector: "Cybersecurity",  last: 318.40, change_pct: -1.85, rel_vol: 1.4, market_cap: "$77B",    rsi: 41.3, signal: ScreenerSignal::Reversal },
    ScreenerRow { symbol: "SMCI", sector: "Hardware",       last: 712.00, change_pct: 7.18,  rel_vol: 4.2, market_cap: "$41B",    rsi: 78.9, signal: ScreenerSignal::Breakout },
    ScreenerRow { symbol: "TSLA", sector: "Auto",           last: 248.50, change_pct: -1.89, rel_vol: 1.7, market_cap: "$789B",   rsi: 36.4, signal: ScreenerSignal::Oversold },
    ScreenerRow { symbol: "ARM",  sector: "Semiconductors", last: 142.65, change_pct: 2.92,  rel_vol: 2.0, market_cap: "$148B",   rsi: 64.7, signal: ScreenerSignal::Squeeze },
    ScreenerRow { symbol: "AVGO", sector: "Semiconductors", last: 1342.10, change_pct: 1.74, rel_vol: 1.3, market_cap: "$626B",   rsi: 58.9, signal: ScreenerSignal::Momentum },
    ScreenerRow { symbol: "SHOP", sector: "Software",       last: 75.40,  change_pct: -3.18, rel_vol: 1.5, market_cap: "$96B",    rsi: 32.7, signal: ScreenerSignal::Reversal },
];

fn render_screener(_window: &mut Window, cx: &mut Context<ContentPanel>) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let bullish = theme.chart_bullish;
    let bearish = theme.chart_bearish;
    let border = theme.border;

    let chip = |label: &'static str, active: bool| {
        let (bg, fg) = if active {
            (theme.primary, theme.primary_foreground)
        } else {
            (theme.muted, theme.muted_foreground)
        };
        div()
            .px_2()
            .py_0p5()
            .rounded(gpui::px(999.))
            .bg(bg)
            .text_color(fg)
            .text_xs()
            .child(label)
    };

    let filter_bar = v_flex()
        .px_3()
        .py_2()
        .gap_2()
        .border_b_1()
        .border_color(border)
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(div().text_sm().font_semibold().child("Stock Screener"))
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{} matches", SCREENER_ROWS.len())),
                ),
        )
        .child(
            h_flex()
                .gap_2()
                .flex_wrap()
                .child(div().text_xs().text_color(muted).child("Universe:"))
                .child(chip("S&P 500", true))
                .child(chip("NASDAQ 100", false))
                .child(chip("Russell 2000", false))
                .child(div().w_2())
                .child(div().text_xs().text_color(muted).child("Cap:"))
                .child(chip("Mega", true))
                .child(chip("Large", true))
                .child(chip("Mid", false)),
        )
        .child(
            h_flex()
                .gap_2()
                .flex_wrap()
                .child(div().text_xs().text_color(muted).child("Signal:"))
                .child(chip("Breakout", true))
                .child(chip("Momentum", true))
                .child(chip("Oversold", true))
                .child(chip("Squeeze", true))
                .child(chip("Reversal", false))
                .child(div().w_2())
                .child(div().text_xs().text_color(muted).child("RVol > 1.0"))
                .child(div().text_xs().text_color(muted).child("· Price > $20")),
        );

    let header_row = h_flex()
        .px_3()
        .py_1p5()
        .gap_2()
        .text_xs()
        .text_color(muted)
        .border_b_1()
        .border_color(border)
        .child(div().w(gpui::px(64.)).child("Symbol"))
        .child(div().flex_1().child("Sector"))
        .child(div().w(gpui::px(72.)).text_right().child("Last"))
        .child(div().w(gpui::px(64.)).text_right().child("Chg %"))
        .child(div().w(gpui::px(56.)).text_right().child("RVol"))
        .child(div().w(gpui::px(56.)).text_right().child("RSI"))
        .child(div().w(gpui::px(72.)).text_right().child("Cap"))
        .child(div().w(gpui::px(80.)).text_right().child("Signal"));

    let rows = v_flex().children(SCREENER_ROWS.iter().map(|r| {
        let chg_color = if r.change_pct >= 0.0 { bullish } else { bearish };
        let (signal_label, signal_color) = match r.signal {
            ScreenerSignal::Breakout => ("BREAKOUT", bullish),
            ScreenerSignal::Oversold => ("OVERSOLD", theme.chart_4),
            ScreenerSignal::Squeeze  => ("SQUEEZE",  theme.chart_5),
            ScreenerSignal::Reversal => ("REVERSAL", theme.chart_3),
            ScreenerSignal::Momentum => ("MOMENTUM", theme.chart_2),
        };
        let rsi_color = if r.rsi >= 70.0 {
            bullish
        } else if r.rsi <= 30.0 {
            bearish
        } else {
            theme.foreground
        };
        h_flex()
            .px_3()
            .py_1p5()
            .gap_2()
            .text_sm()
            .border_b_1()
            .border_color(border)
            .child(div().w(gpui::px(64.)).font_semibold().child(r.symbol))
            .child(div().flex_1().text_color(muted).child(r.sector))
            .child(
                div()
                    .w(gpui::px(72.))
                    .text_right()
                    .child(format!("{:.2}", r.last)),
            )
            .child(
                div()
                    .w(gpui::px(64.))
                    .text_right()
                    .text_color(chg_color)
                    .child(format!("{:+.2}%", r.change_pct)),
            )
            .child(
                div()
                    .w(gpui::px(56.))
                    .text_right()
                    .child(format!("{:.1}x", r.rel_vol)),
            )
            .child(
                div()
                    .w(gpui::px(56.))
                    .text_right()
                    .text_color(rsi_color)
                    .child(format!("{:.0}", r.rsi)),
            )
            .child(
                div()
                    .w(gpui::px(72.))
                    .text_right()
                    .text_color(muted)
                    .child(r.market_cap),
            )
            .child(
                div()
                    .w(gpui::px(80.))
                    .text_right()
                    .child(
                        div()
                            .px_1p5()
                            .py_0p5()
                            .rounded(gpui::px(3.))
                            .text_xs()
                            .font_semibold()
                            .text_color(signal_color)
                            .border_1()
                            .border_color(signal_color)
                            .child(signal_label),
                    ),
            )
    }));

    v_flex().w_full().child(filter_bar).child(header_row).child(rows)
}

// ============================================================================
// Geopolitics
// ============================================================================

#[derive(Clone, Copy)]
enum GeoSeverity { Critical, High, Medium, Low }

#[derive(Clone, Copy)]
enum GeoRegion {
    MiddleEast,
    Europe,
    AsiaPacific,
    Americas,
    Africa,
}

struct GeoEvent {
    when: &'static str,
    region: GeoRegion,
    severity: GeoSeverity,
    headline: &'static str,
    summary: &'static str,
    asset_impact: &'static [(&'static str, f64)],
}

const GEO_EVENTS: &[GeoEvent] = &[
    GeoEvent {
        when: "12m ago",
        region: GeoRegion::MiddleEast,
        severity: GeoSeverity::Critical,
        headline: "Strikes near Iranian oil terminal at Kharg Island",
        summary: "Reuters reports two explosions near loading jetties. Tankers reroute. Brent +3.4% intraday; Saudi defense cabinet convenes.",
        asset_impact: &[("BRENT", 3.42), ("XLE", 1.85), ("SPX", -0.62), ("USDJPY", -0.41)],
    },
    GeoEvent {
        when: "47m ago",
        region: GeoRegion::Europe,
        severity: GeoSeverity::High,
        headline: "EU agrees on 14th sanctions package against Russia",
        summary: "Council greenlights LNG transshipment ban via EU ports plus secondary sanctions on third-country re-exporters. Brussels signals more measures by Q3.",
        asset_impact: &[("EUR", -0.18), ("TTF-GAS", 2.10), ("STOXX600", -0.34)],
    },
    GeoEvent {
        when: "1h ago",
        region: GeoRegion::AsiaPacific,
        severity: GeoSeverity::High,
        headline: "Taiwan reports record PLA Navy incursion across median line",
        summary: "Taiwan MND says 28 PLAN vessels and 47 aircraft crossed the strait’s median line overnight. Pentagon comments expected pre-market.",
        asset_impact: &[("TSM", -2.12), ("SOXX", -1.04), ("USDTWD", 0.38), ("XAU", 0.62)],
    },
    GeoEvent {
        when: "2h ago",
        region: GeoRegion::Americas,
        severity: GeoSeverity::Medium,
        headline: "Mexico court ruling threatens to delay USMCA review",
        summary: "Constitutional ruling on energy nationalization complicates U.S.–Mexico negotiations ahead of 2026 USMCA review. Auto and energy supply-chain risk repriced.",
        asset_impact: &[("MXN", -0.58), ("EWW", -0.94), ("F", -0.41)],
    },
    GeoEvent {
        when: "3h ago",
        region: GeoRegion::Europe,
        severity: GeoSeverity::Medium,
        headline: "France downgraded to AA- by S&P; spreads widen vs Bunds",
        summary: "Cited fiscal slippage and political fragmentation. OAT-Bund 10y spread blows out to 78bp, the widest since 2012.",
        asset_impact: &[("CAC40", -0.88), ("OAT-BUND", 7.80), ("EUR", -0.27)],
    },
    GeoEvent {
        when: "5h ago",
        region: GeoRegion::AsiaPacific,
        severity: GeoSeverity::Low,
        headline: "BoJ governor Ueda signals patience on next hike",
        summary: "Speech at IMF: data-dependent, but “normalization remains the direction.” Yen weakens through 158 against the dollar.",
        asset_impact: &[("USDJPY", 0.74), ("NKY", 1.12), ("JGB10Y", 1.20)],
    },
    GeoEvent {
        when: "Yesterday",
        region: GeoRegion::Africa,
        severity: GeoSeverity::Medium,
        headline: "Niger junta orders French uranium operator to suspend ops",
        summary: "Orano-operated Arlit mine paused. Uranium spot up 4%. EDF reviews fuel-supply contingencies.",
        asset_impact: &[("URA", 4.02), ("CCJ", 3.18), ("XAU", 0.41)],
    },
    GeoEvent {
        when: "Yesterday",
        region: GeoRegion::MiddleEast,
        severity: GeoSeverity::High,
        headline: "Houthi attacks resume in Red Sea after 2-week lull",
        summary: "Two more bulkers struck near Bab el-Mandeb. CMA CGM and Maersk extend Cape of Good Hope reroutes through end of quarter.",
        asset_impact: &[("FBX-CN-EU", 8.40), ("ZIM", 5.12), ("BRENT", 1.20)],
    },
];

fn render_geopolitics(_window: &mut Window, cx: &mut Context<ContentPanel>) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let bullish = theme.chart_bullish;
    let bearish = theme.chart_bearish;
    let border = theme.border;

    let region_label_color = |r: GeoRegion| -> (&'static str, gpui::Hsla) {
        match r {
            GeoRegion::MiddleEast  => ("MIDDLE EAST",  theme.chart_3),
            GeoRegion::Europe      => ("EUROPE",       theme.chart_2),
            GeoRegion::AsiaPacific => ("ASIA-PACIFIC", theme.chart_5),
            GeoRegion::Americas    => ("AMERICAS",     theme.chart_1),
            GeoRegion::Africa      => ("AFRICA",       theme.chart_4),
        }
    };

    let severity_label_color = |s: GeoSeverity| -> (&'static str, gpui::Hsla) {
        match s {
            GeoSeverity::Critical => ("CRITICAL", theme.chart_bearish),
            GeoSeverity::High     => ("HIGH",     theme.chart_4),
            GeoSeverity::Medium   => ("MEDIUM",   theme.chart_5),
            GeoSeverity::Low      => ("LOW",      theme.chart_bullish),
        }
    };

    let header = h_flex()
        .px_3()
        .py_2()
        .items_center()
        .gap_2()
        .border_b_1()
        .border_color(border)
        .child(div().size_2().rounded_full().bg(theme.chart_4))
        .child(div().text_sm().font_semibold().child("Geopolitical Watch"))
        .child(div().flex_1())
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child("Live · macro & cross-asset impact"),
        );

    let events = v_flex()
        .w_full()
        .p_2()
        .gap_2()
        .children(GEO_EVENTS.iter().enumerate().map(|(idx, e)| {
            let (region_label, region_color) = region_label_color(e.region);
            let (sev_label, sev_color) = severity_label_color(e.severity);
            v_flex()
                .id(SharedString::from(format!("geo-row-{idx}")))
                .px_3()
                .py_2()
                .gap_2()
                .rounded(gpui::px(6.))
                .border_1()
                .border_color(border)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .text_xs()
                        .child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded(gpui::px(3.))
                                .bg(sev_color)
                                .text_color(theme.background)
                                .font_semibold()
                                .child(sev_label),
                        )
                        .child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded(gpui::px(3.))
                                .border_1()
                                .border_color(region_color)
                                .text_color(region_color)
                                .font_semibold()
                                .child(region_label),
                        )
                        .child(div().flex_1())
                        .child(div().text_color(muted).child(e.when)),
                )
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(theme.foreground)
                        .child(e.headline),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(e.summary),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .children(e.asset_impact.iter().map(|(asset, pct)| {
                            let color = if *pct >= 0.0 { bullish } else { bearish };
                            h_flex()
                                .px_1p5()
                                .py_0p5()
                                .gap_1()
                                .rounded(gpui::px(3.))
                                .bg(theme.muted)
                                .text_xs()
                                .child(
                                    div()
                                        .text_color(theme.foreground)
                                        .font_semibold()
                                        .child(*asset),
                                )
                                .child(
                                    div()
                                        .text_color(color)
                                        .child(format!("{:+.2}%", pct)),
                                )
                        })),
                )
        }));

    v_flex().w_full().child(header).child(events)
}

// ============================================================================
// Details
// ============================================================================

fn render_details(_window: &mut Window, cx: &mut Context<ContentPanel>) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .w_full()
        .p_3()
        .gap_3()
        .child(
            v_flex()
                .gap_1()
                .child(div().text_lg().font_semibold().child("AAPL"))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("Apple Inc. · Common Stock · USD"),
                ),
        )
        .child(
            DescriptionList::new()
                .columns(2)
                .label_width(gpui::px(140.))
                .item("Last", "$185.32", 1)
                .item("Change", "+$2.25 (+1.23%)", 1)
                .item("Open", "$183.07", 1)
                .item("Prev Close", "$183.07", 1)
                .item("Day Range", "$182.41 – $185.97", 1)
                .item("52w Range", "$164.08 – $237.49", 1)
                .item("Volume", "45.6M", 1)
                .item("Avg Volume", "58.2M", 1)
                .item("Market Cap", "$2.85T", 1)
                .item("P/E (TTM)", "28.94", 1)
                .item("EPS (TTM)", "$6.41", 1)
                .item("Div Yield", "0.52%", 1)
                .item("Beta", "1.29", 1)
                .item("Exchange", "NASDAQ", 1),
        )
}

// ============================================================================
// News Feed
// ============================================================================

struct NewsItem {
    time: &'static str,
    tag: &'static str,
    headline: &'static str,
    source: &'static str,
    body: &'static str,
    link: &'static str,
}

const NEWS: &[NewsItem] = &[
    NewsItem { time: "10:32", tag: "AAPL",  headline: "Apple reports Q2 earnings beat, services revenue up 14%", source: "Reuters",   link: "https://reuters.com/aapl-q2",      body: "Apple Inc. reported fiscal Q2 revenue of $94.8B, beating consensus estimates of $93.2B. Services revenue jumped 14% YoY to $24.2B, a new all-time high. iPhone revenue was flat YoY at $51.3B amid softer demand in Greater China. The company guided to low single-digit revenue growth for Q3 and authorized an additional $110B in share buybacks." },
    NewsItem { time: "10:15", tag: "FED",   headline: "Powell signals patient approach on rate cuts",            source: "Bloomberg", link: "https://bloomberg.com/fed-powell", body: "Fed Chair Jerome Powell, speaking at a Stanford conference, indicated that policymakers are in no rush to cut rates and want more evidence that inflation is sustainably moving toward the 2% target. Markets pared back expectations for the first cut to September, with the probability of a June cut now at 18%." },
    NewsItem { time: "09:58", tag: "NVDA",  headline: "NVIDIA hits new all-time high on AI chip demand",         source: "CNBC",      link: "https://cnbc.com/nvda-ath",       body: "NVIDIA shares hit a new intraday high of $878.50 after Morgan Stanley raised its price target to $1,100, citing accelerating Blackwell GPU pre-orders from hyperscalers. Microsoft, Meta, and Google are reportedly tripling their AI infrastructure capex through 2025." },
    NewsItem { time: "09:42", tag: "META",  headline: "Meta unveils next-gen Ray-Ban smart glasses",             source: "The Verge", link: "https://theverge.com/meta-rayban",  body: "Meta announced the next generation of its Ray-Ban Stories smart glasses, featuring an integrated display, 12-hour battery life, and on-device Llama 4 inference. The product will start shipping in Q4 at $499 and is positioned as a direct competitor to Apple Vision Pro for ambient computing." },
    NewsItem { time: "09:30", tag: "MKT",   headline: "S&P 500 opens flat as traders await CPI data",            source: "WSJ",       link: "https://wsj.com/sp500-flat",       body: "U.S. stocks opened little changed on Tuesday as traders awaited Wednesday's CPI report, which is expected to show core inflation rising 0.3% MoM. Treasury yields edged up, with the 10-year hovering near 4.40%. Dollar index was steady at 105.2." },
    NewsItem { time: "09:14", tag: "TSLA",  headline: "Tesla deliveries miss estimates; stock down 2% premkt",   source: "Reuters",   link: "https://reuters.com/tsla-deliveries", body: "Tesla reported Q1 deliveries of 386,810 vehicles, well below the consensus estimate of 449,080. The company cited the Berlin gigafactory arson, Red Sea shipping disruptions, and the Model 3 refresh ramp as headwinds. Year-over-year deliveries fell 8.5%, the first annual decline since 2020." },
    NewsItem { time: "08:55", tag: "BTC",   headline: "Bitcoin steady at $68k as ETF inflows resume",            source: "CoinDesk",  link: "https://coindesk.com/btc-etf",     body: "Bitcoin held steady at $68,200 as spot BTC ETFs recorded $312M of net inflows on Monday, ending a five-day outflow streak. BlackRock's IBIT led with $145M of inflows, followed by Fidelity's FBTC. Total ETF AUM has now surpassed $58B since launch in January." },
    NewsItem { time: "08:30", tag: "ECON",  headline: "Initial jobless claims fall to 218k, below estimates",    source: "BLS",       link: "https://bls.gov/jobless-claims",   body: "Initial jobless claims fell to 218,000 in the week ended April 27, below consensus of 232,000 and the lowest reading since February. The 4-week moving average ticked down to 213,250. Continuing claims rose modestly to 1.78M, suggesting a labor market that remains tight but is gradually cooling." },
    NewsItem { time: "08:12", tag: "OIL",   headline: "Brent crude rises 1.2% on Middle East supply concerns",   source: "Bloomberg", link: "https://bloomberg.com/oil-mideast", body: "Brent crude rose 1.2% to $89.45/bbl after reports of Israeli airstrikes near a key Iranian oil terminal. Goldman Sachs raised its summer Brent forecast to $95/bbl, citing tighter OPEC+ compliance and stronger-than-expected gasoline demand heading into driving season." },
    NewsItem { time: "07:45", tag: "GOOGL", headline: "Alphabet announces $70B share buyback plan",              source: "FT",        link: "https://ft.com/googl-buyback",     body: "Alphabet's board authorized a $70B share repurchase program and declared its first-ever quarterly dividend of $0.20/share. The buyback represents about 4% of the company's market cap and follows similar dividend initiations from Meta earlier this year. Shares jumped 14% in after-hours trading on the announcement." },
];

fn render_news(_window: &mut Window, cx: &mut Context<ContentPanel>) -> impl IntoElement {
    let theme = cx.theme();
    let hover_bg = theme.accent;
    v_flex()
        .w_full()
        .p_2()
        .gap_1()
        .children(NEWS.iter().enumerate().map(|(idx, n)| {
            v_flex()
                .id(SharedString::from(format!("news-row-{idx}")))
                .px_2()
                .py_2()
                .gap_1()
                .rounded(gpui::px(4.))
                .border_b_1()
                .border_color(theme.border)
                .cursor_pointer()
                .hover(|s| s.bg(hover_bg))
                .on_click(cx.listener(move |_this, _ev, window, cx| {
                    open_news_dialog(n, window, cx);
                }))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(div().child(n.time))
                        .child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded(gpui::px(3.))
                                .bg(theme.accent)
                                .text_color(theme.accent_foreground)
                                .text_xs()
                                .child(n.tag),
                        )
                        .child(div().flex_1())
                        .child(div().child(n.source)),
                )
                .child(div().text_sm().text_color(theme.foreground).child(n.headline))
                .child(
                    h_flex().pt_1().child(ask_ai_button(format!("news-ask-{idx}"), n)),
                )
        }))
}

/// Build the "Ask AI" prompt for a news item — used by both the inline button
/// and the popup dialog button. The chat input is `auto_grow`, so newlines
/// are allowed and the box expands to fit the prefilled text.
fn ask_ai_prompt(n: &NewsItem) -> SharedString {
    SharedString::from(format!(
        "What is the implication behind this news and how it affects my position?\n\n\
         {} ({})\nSource: {}",
        n.headline, n.tag, n.link,
    ))
}

fn ask_ai_button(id: impl Into<SharedString>, n: &'static NewsItem) -> Button {
    Button::new(id.into())
        .label("Ask AI")
        .small()
        .ghost()
        .icon(gpui_component::IconName::Bot)
        .on_click(move |_, window, cx| {
            // Don't bubble to the row's on_click which opens the dialog.
            cx.stop_propagation();
            window.dispatch_action(Box::new(crate::top_bar::AskAi(ask_ai_prompt(n))), cx);
        })
}

fn open_news_dialog(n: &'static NewsItem, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let accent = theme.accent;
        let accent_fg = theme.accent_foreground;
        let border = theme.border;
        dialog
            .title(SharedString::from(n.headline))
            .max_w(gpui::px(560.))
            .button_props(
                DialogButtonProps::default()
                    .show_cancel(true)
                    .ok_text("Open Source")
                    .on_ok(move |_, window, cx| {
                        window.push_notification(
                            Notification::info(SharedString::from(format!(
                                "Opening source: {}",
                                n.link
                            )))
                            .title("News source"),
                            cx,
                        );
                        true
                    }),
            )
            .child(
                v_flex()
                    .px_4()
                    .pb_4()
                    .gap_3()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .text_xs()
                            .text_color(muted)
                            .child(div().child(n.time))
                            .child(
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded(gpui::px(3.))
                                    .bg(accent)
                                    .text_color(accent_fg)
                                    .child(n.tag),
                            )
                            .child(div().child("·"))
                            .child(div().child(n.source)),
                    )
                    .child(
                        div()
                            .pt_2()
                            .border_t_1()
                            .border_color(border)
                            .text_sm()
                            .child(n.body),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .text_color(muted)
                            .child("Source:")
                            .child(
                                Link::new("news-link")
                                    .href(n.link)
                                    .child(n.link),
                            ),
                    )
                    .child(
                        h_flex().pt_1().child(
                            Button::new("news-dialog-ask")
                                .label("Ask AI")
                                .small()
                                .primary()
                                .icon(gpui_component::IconName::Bot)
                                .on_click(move |_, window, cx| {
                                    window.dispatch_action(
                                        Box::new(crate::top_bar::AskAi(ask_ai_prompt(n))),
                                        cx,
                                    );
                                    window.close_all_dialogs(cx);
                                }),
                        ),
                    ),
            )
    });
}

// ============================================================================
// Portfolio
// ============================================================================

struct Holding {
    symbol: &'static str,
    shares: u32,
    cost: f64,
    last: f64,
}

const PORTFOLIO: &[Holding] = &[
    Holding { symbol: "AAPL",  shares: 100, cost: 158.20, last: 185.32 },
    Holding { symbol: "MSFT",  shares: 50,  cost: 310.45, last: 378.45 },
    Holding { symbol: "NVDA",  shares: 25,  cost: 412.60, last: 875.21 },
    Holding { symbol: "GOOGL", shares: 80,  cost: 138.10, last: 142.18 },
    Holding { symbol: "TSLA",  shares: 40,  cost: 275.00, last: 248.50 },
    Holding { symbol: "BRK.B", shares: 30,  cost: 380.20, last: 412.65 },
];

fn render_portfolio(_window: &mut Window, cx: &mut Context<ContentPanel>) -> impl IntoElement {
    let theme = cx.theme();
    let bullish = theme.chart_bullish;
    let bearish = theme.chart_bearish;
    let muted = theme.muted_foreground;
    let border = theme.border;

    let total_cost: f64 = PORTFOLIO.iter().map(|h| h.shares as f64 * h.cost).sum();
    let total_value: f64 = PORTFOLIO.iter().map(|h| h.shares as f64 * h.last).sum();
    let total_pl = total_value - total_cost;
    let total_pl_pct = (total_pl / total_cost) * 100.0;
    let total_color = if total_pl >= 0.0 { bullish } else { bearish };

    v_flex()
        .w_full()
        .p_2()
        .gap_2()
        .child(
            // Summary header
            v_flex()
                .px_3()
                .py_2()
                .gap_1()
                .rounded(gpui::px(6.))
                .bg(theme.muted)
                .child(
                    h_flex()
                        .items_baseline()
                        .gap_2()
                        .child(div().text_xs().text_color(muted).child("Total Value"))
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_lg()
                                .font_semibold()
                                .child(format!("${:.2}", total_value)),
                        ),
                )
                .child(
                    h_flex()
                        .items_baseline()
                        .gap_2()
                        .text_sm()
                        .child(div().text_xs().text_color(muted).child("P/L"))
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_color(total_color)
                                .child(format!("{:+.2} ({:+.2}%)", total_pl, total_pl_pct)),
                        ),
                ),
        )
        .child(
            h_flex()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(muted)
                .border_b_1()
                .border_color(border)
                .child(div().w_16().child("Symbol"))
                .child(div().w_12().text_right().child("Sh"))
                .child(div().flex_1().text_right().child("Cost"))
                .child(div().flex_1().text_right().child("Last"))
                .child(div().flex_1().text_right().child("P/L")),
        )
        .children(PORTFOLIO.iter().map(|h| {
            let value = h.shares as f64 * h.last;
            let cost_basis = h.shares as f64 * h.cost;
            let pl = value - cost_basis;
            let pl_pct = (pl / cost_basis) * 100.0;
            let color = if pl >= 0.0 { bullish } else { bearish };
            h_flex()
                .px_2()
                .py_1()
                .text_sm()
                .child(div().w_16().font_semibold().child(h.symbol))
                .child(div().w_12().text_right().child(h.shares.to_string()))
                .child(div().flex_1().text_right().text_color(muted).child(format!("{:.2}", h.cost)))
                .child(div().flex_1().text_right().child(format!("{:.2}", h.last)))
                .child(
                    div()
                        .flex_1()
                        .text_right()
                        .text_color(color)
                        .child(format!("{:+.0} ({:+.1}%)", pl, pl_pct)),
                )
        }))
}

// ============================================================================
// Notifications
// ============================================================================

#[derive(Clone, Copy)]
enum NotifKind { Alert, Fill, News, Warn }

struct Notif {
    when: &'static str,
    kind: NotifKind,
    text: &'static str,
}

const NOTIFS: &[Notif] = &[
    Notif { when: "now",    kind: NotifKind::Alert, text: "AAPL crossed above $185.00 (price alert)" },
    Notif { when: "2m",     kind: NotifKind::Fill,  text: "Order filled · BUY 100 NVDA @ $873.50" },
    Notif { when: "5m",     kind: NotifKind::News,  text: "GOOGL beats earnings by $0.12" },
    Notif { when: "12m",    kind: NotifKind::Warn,  text: "TSLA dropped -2% in last 30m" },
    Notif { when: "23m",    kind: NotifKind::Alert, text: "BTC reached daily high $68,420" },
    Notif { when: "45m",    kind: NotifKind::Fill,  text: "Order partial fill · SELL 50/200 META @ $501.80" },
    Notif { when: "1h",     kind: NotifKind::News,  text: "FOMC minutes released — no surprises" },
    Notif { when: "2h",     kind: NotifKind::Warn,  text: "VIX up +8.3% — elevated volatility" },
];

fn render_notifications(_window: &mut Window, cx: &mut Context<ContentPanel>) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let border = theme.border;
    v_flex()
        .w_full()
        .p_2()
        .children(NOTIFS.iter().map(|n| {
            let (label, color) = match n.kind {
                NotifKind::Alert => ("ALERT", theme.info),
                NotifKind::Fill  => ("FILL",  theme.chart_bullish),
                NotifKind::News  => ("NEWS",  theme.accent),
                NotifKind::Warn  => ("WARN",  theme.chart_bearish),
            };
            h_flex()
                .px_2()
                .py_2()
                .gap_3()
                .items_start()
                .border_b_1()
                .border_color(border)
                .child(
                    div()
                        .w_16()
                        .text_xs()
                        .font_semibold()
                        .text_color(color)
                        .child(label),
                )
                .child(div().flex_1().text_sm().child(n.text))
                .child(div().w_10().text_right().text_xs().text_color(muted).child(n.when))
        }))
}

// ============================================================================
// Smart Money
// ============================================================================

#[derive(Clone, Copy)]
enum FlowKind { Insider, Whale, Option }

struct Flow {
    kind: FlowKind,
    actor: &'static str,
    action: &'static str,
    detail: &'static str,
    notional: &'static str,
    bullish: Option<bool>,
}

const FLOWS: &[Flow] = &[
    Flow { kind: FlowKind::Insider, actor: "Tim Cook (CEO)",       action: "SOLD",   detail: "50,000 AAPL @ $184.21",       notional: "-$9.21M",  bullish: Some(false) },
    Flow { kind: FlowKind::Whale,   actor: "0x4f3a…b21c",          action: "MOVED",  detail: "1,250 BTC → Coinbase",         notional: " $86.5M",  bullish: None },
    Flow { kind: FlowKind::Option,  actor: "Unusual Sweep",         action: "CALL",   detail: "AAPL Mar 200C · 12,000 ct",   notional: " $3.4M",   bullish: Some(true) },
    Flow { kind: FlowKind::Insider, actor: "Elon Musk (CEO)",       action: "FILED",  detail: "Form 4 · 100K TSLA buy",      notional: "+$24.85M", bullish: Some(true) },
    Flow { kind: FlowKind::Option,  actor: "Block Trade",            action: "PUT",    detail: "QQQ May 420P · 8,500 ct",     notional: " $2.1M",   bullish: Some(false) },
    Flow { kind: FlowKind::Whale,   actor: "0x7a91…ff04",          action: "DUMPED", detail: "42,000 ETH → Binance",         notional: " $144M",   bullish: Some(false) },
    Flow { kind: FlowKind::Insider, actor: "Satya Nadella (CEO)",   action: "BOUGHT", detail: "5,000 MSFT @ $377.10",         notional: "+$1.89M",  bullish: Some(true) },
    Flow { kind: FlowKind::Option,  actor: "Sweep",                 action: "CALL",   detail: "NVDA Apr 900C · 18,000 ct",   notional: " $5.8M",   bullish: Some(true) },
    Flow { kind: FlowKind::Whale,   actor: "0xc04e…118d",          action: "ACCUM",  detail: "920 BTC over 6h",              notional: " $63.6M",  bullish: Some(true) },
    Flow { kind: FlowKind::Insider, actor: "Andy Jassy (CEO)",      action: "SOLD",   detail: "12,500 AMZN @ $178.40",        notional: "-$2.23M",  bullish: Some(false) },
];

fn render_smart_money(_window: &mut Window, cx: &mut Context<ContentPanel>) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let bullish = theme.chart_bullish;
    let bearish = theme.chart_bearish;
    let border = theme.border;
    v_flex()
        .w_full()
        .p_2()
        .children(FLOWS.iter().map(|f| {
            let (kind_label, kind_color) = match f.kind {
                FlowKind::Insider => ("INSIDER", theme.chart_4),
                FlowKind::Whale   => ("WHALE",   theme.chart_5),
                FlowKind::Option  => ("OPT",     theme.chart_2),
            };
            let notional_color = match f.bullish {
                Some(true)  => bullish,
                Some(false) => bearish,
                None        => muted,
            };
            v_flex()
                .px_2()
                .py_2()
                .gap_1()
                .border_b_1()
                .border_color(border)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .w_16()
                                .text_xs()
                                .font_semibold()
                                .text_color(kind_color)
                                .child(kind_label),
                        )
                        .child(div().text_sm().font_semibold().child(f.actor))
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .text_color(notional_color)
                                .child(f.notional),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .text_color(muted)
                        .child(div().w_16().child(f.action))
                        .child(div().flex_1().child(f.detail)),
                )
        }))
}

// ============================================================================
// AI Chat
// ============================================================================

#[derive(Clone, Copy)]
enum Speaker { User, Assistant }

struct ChatMsg {
    speaker: Speaker,
    text: &'static str,
}

const CHAT: &[ChatMsg] = &[
    ChatMsg { speaker: Speaker::User,      text: "What's the macro outlook for this week?" },
    ChatMsg { speaker: Speaker::Assistant, text: "Markets are watching CPI on Wednesday and the Fed minutes Thursday. Consensus expects core CPI at +0.3% MoM. A hotter print likely pushes back rate-cut expectations and pressures growth names." },
    ChatMsg { speaker: Speaker::User,      text: "How does NVDA look technically?" },
    ChatMsg { speaker: Speaker::Assistant, text: "NVDA broke above its 50-day MA at $850 on heavy volume. RSI is at 68 — approaching overbought but not yet stretched. Watch the $880 resistance from late March; a clean break opens $920+." },
    ChatMsg { speaker: Speaker::User,      text: "Any unusual options activity?" },
    ChatMsg { speaker: Speaker::Assistant, text: "Yes — large call sweeps on AAPL Mar 200C (12k contracts) and NVDA Apr 900C (18k contracts) crossed in the last hour. Both bid-side, suggesting bullish positioning into earnings season." },
];

fn render_ai_chat(
    input: &Entity<InputState>,
    _window: &mut Window,
    cx: &mut Context<ContentPanel>,
) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let border = theme.border;
    let bg_user = theme.primary;
    let fg_user = theme.primary_foreground;
    let bg_assistant = theme.muted;
    let fg_assistant = theme.foreground;

    v_flex()
        .w_full()
        .child(
            // Header
            h_flex()
                .px_3()
                .py_2()
                .gap_2()
                .items_center()
                .border_b_1()
                .border_color(border)
                .child(
                    div()
                        .size_2()
                        .rounded_full()
                        .bg(theme.chart_bullish),
                )
                .child(div().text_sm().font_semibold().child("AI Assistant"))
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("Sonnet · ready"),
                ),
        )
        .child(
            // Message list — fills remaining space, scrollable
            div()
                .id("chat-msgs")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_3()
                .child(
                    v_flex().gap_3().children(CHAT.iter().map(|m| {
                        let (bg, fg, align) = match m.speaker {
                            Speaker::User => (bg_user, fg_user, true),
                            Speaker::Assistant => (bg_assistant, fg_assistant, false),
                        };
                        h_flex()
                            .w_full()
                            .when(align, |this| this.justify_end())
                            .child(
                                div()
                                    .max_w(gpui::px(420.))
                                    .px_3()
                                    .py_2()
                                    .rounded(gpui::px(8.))
                                    .bg(bg)
                                    .text_color(fg)
                                    .text_sm()
                                    .child(m.text),
                            )
                    })),
                ),
        )
        .child(
            // Real input bar — bound to the panel's InputState so users can type.
            h_flex()
                .px_3()
                .py_2()
                .gap_2()
                .items_center()
                .border_t_1()
                .border_color(border)
                .child(div().flex_1().child(Input::new(input)))
                .child(Button::new("send").label("Send").small().primary()),
        )
}

// ============================================================================
// Position
// ============================================================================

#[derive(Clone, Copy)]
struct PositionRow {
    symbol: &'static str,
    side: &'static str,
    qty: i32,
    entry: f64,
    last: f64,
}

const POSITIONS: &[PositionRow] = &[
    PositionRow { symbol: "AAPL", side: "LONG",  qty: 200, entry: 180.40, last: 192.15 },
    PositionRow { symbol: "NVDA", side: "LONG",  qty: 50,  entry: 812.00, last: 875.21 },
    PositionRow { symbol: "TSLA", side: "SHORT", qty: 75,  entry: 268.30, last: 248.50 },
    PositionRow { symbol: "MSFT", side: "LONG",  qty: 90,  entry: 410.20, last: 423.85 },
];

fn render_position(_window: &mut Window, cx: &mut Context<ContentPanel>) -> impl IntoElement {
    let theme = cx.theme();
    let bullish = theme.chart_bullish;
    let bearish = theme.chart_bearish;
    let muted = theme.muted_foreground;
    let border = theme.border;

    let header = h_flex()
        .px_3()
        .py_1p5()
        .gap_2()
        .text_xs()
        .text_color(muted)
        .border_b_1()
        .border_color(border)
        .child(div().w(gpui::px(72.)).child("Symbol"))
        .child(div().w(gpui::px(56.)).child("Side"))
        .child(div().w(gpui::px(56.)).text_right().child("Qty"))
        .child(div().w(gpui::px(80.)).text_right().child("Entry"))
        .child(div().w(gpui::px(80.)).text_right().child("Last"))
        .child(div().flex_1().text_right().child("P/L"));

    let rows = POSITIONS.iter().map(|p| {
        let pl = (p.last - p.entry) * p.qty as f64
            * if p.side == "SHORT" { -1.0 } else { 1.0 };
        let pl_color = if pl >= 0.0 { bullish } else { bearish };
        let side_color = if p.side == "LONG" { bullish } else { bearish };
        h_flex()
            .px_3()
            .py_1()
            .gap_2()
            .text_xs()
            .border_b_1()
            .border_color(border)
            .child(div().w(gpui::px(72.)).font_semibold().child(p.symbol))
            .child(div().w(gpui::px(56.)).text_color(side_color).child(p.side))
            .child(div().w(gpui::px(56.)).text_right().child(format!("{}", p.qty)))
            .child(
                div()
                    .w(gpui::px(80.))
                    .text_right()
                    .text_color(muted)
                    .child(format!("{:.2}", p.entry)),
            )
            .child(
                div()
                    .w(gpui::px(80.))
                    .text_right()
                    .child(format!("{:.2}", p.last)),
            )
            .child(
                div()
                    .flex_1()
                    .text_right()
                    .text_color(pl_color)
                    .child(format!("{:+.2}", pl)),
            )
    });

    v_flex().w_full().child(header).children(rows)
}

// ============================================================================
// Execution
// ============================================================================

fn render_execution(
    inputs: &ExecutionInputs,
    _window: &mut Window,
    cx: &mut Context<ContentPanel>,
) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let bullish = theme.chart_bullish;
    let bearish = theme.chart_bearish;

    let field = |label: &'static str, input: &Entity<InputState>| {
        v_flex()
            .gap_1()
            .child(div().text_xs().text_color(muted).child(label))
            .child(Input::new(input).small())
    };

    let symbol = inputs.symbol.clone();
    let qty = inputs.quantity.clone();
    let limit = inputs.limit.clone();

    let buy = {
        let symbol = symbol.clone();
        let qty = qty.clone();
        let limit = limit.clone();
        Button::new("buy").label("BUY").small().primary().on_click(
            move |_, window, cx| place_order("BUY", &symbol, &qty, &limit, window, cx),
        )
    };
    let sell = {
        let symbol = symbol.clone();
        let qty = qty.clone();
        let limit = limit.clone();
        Button::new("sell").label("SELL").small().primary().on_click(
            move |_, window, cx| place_order("SELL", &symbol, &qty, &limit, window, cx),
        )
    };

    v_flex()
        .w_full()
        .p_3()
        .gap_3()
        .child(div().text_sm().font_semibold().child("Quick Order"))
        .child(field("Symbol", &inputs.symbol))
        .child(field("Quantity", &inputs.quantity))
        .child(field("Limit Price", &inputs.limit))
        .child(
            h_flex()
                .gap_2()
                .child(div().bg(bullish).rounded(gpui::px(4.)).child(buy))
                .child(div().bg(bearish).rounded(gpui::px(4.)).child(sell)),
        )
}

fn place_order(
    side: &'static str,
    symbol: &Entity<InputState>,
    qty: &Entity<InputState>,
    limit: &Entity<InputState>,
    window: &mut Window,
    cx: &mut App,
) {
    let symbol_str = symbol.read(cx).value();
    let qty_str = qty.read(cx).value();
    let limit_str = limit.read(cx).value();

    if symbol_str.trim().is_empty() || qty_str.trim().is_empty() {
        window.push_notification(
            Notification::warning("Symbol and quantity are required").title("Order rejected"),
            cx,
        );
        return;
    }

    let summary = SharedString::from(format!(
        "{side} {qty_str} {sym} @ {limit_str}",
        sym = symbol_str.trim(),
        qty_str = qty_str.trim(),
        limit_str = limit_str.trim(),
    ));
    window.push_notification(
        Notification::success(summary).title("Order placed"),
        cx,
    );
}

// Convenience re-export for callers that still use the old `PanelKind` name.
pub type PanelKind = Kind;
pub const PANEL_KINDS: &[Kind] = Kind::ALL;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    chart::{CandlestickChart, LineChart},
    description_list::DescriptionList,
    dock::{Panel, PanelEvent, PanelView, TabPanel, register_panel},
    h_flex,
    input::{Input, InputState},
    v_flex,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Watchlist,
    Chart,
    Details,
    NewsFeed,
    Portfolio,
    Notification,
    SmartMoney,
    AiChat,
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
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            Kind::NewsFeed => "News Feed",
            Kind::SmartMoney => "Smart Money",
            Kind::AiChat => "AI Chat",
            other => other.id(),
        }
    }

    pub fn from_id(id: &str) -> Option<Kind> {
        Self::ALL.iter().copied().find(|k| k.id() == id)
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
            Box::new(cx.new(|cx| ContentPanel::new(kind, window, cx)))
        });
    }
}

pub fn build_kind(kind: Kind, window: &mut Window, cx: &mut App) -> Arc<dyn PanelView> {
    Arc::new(cx.new(|cx| ContentPanel::new(kind, window, cx)))
}

pub struct ContentPanel {
    kind: Kind,
    focus_handle: FocusHandle,
    parent_tab_panel: Option<WeakEntity<TabPanel>>,
    chat_input: Option<Entity<InputState>>,
    _focus_subscription: Subscription,
}

impl ContentPanel {
    pub fn new(kind: Kind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        // Fires on every click that lands focus into this panel — including clicks on the
        // body of an already-active tab (which `set_active` misses).
        let _focus_subscription = cx.on_focus_in(&focus_handle, window, |this, _window, cx| {
            this.mark_focused(cx);
        });
        // Only the AI chat panel has a real input; other kinds don't pay the InputState cost.
        let chat_input = matches!(kind, Kind::AiChat).then(|| {
            cx.new(|cx| InputState::new(window, cx).placeholder("Ask anything…"))
        });
        Self {
            kind,
            focus_handle,
            parent_tab_panel: None,
            chat_input,
            _focus_subscription,
        }
    }

    fn mark_focused(&self, cx: &mut App) {
        let Some(tab_panel) = self.parent_tab_panel.clone() else {
            return;
        };
        let global = cx.global::<LastFocusedTabPanel>().0.clone();
        *global.borrow_mut() = Some(tab_panel);
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
            Kind::Chart => render_chart(window, cx).into_any_element(),
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
        };
        // AiChat manages its own internal scroll region (so its input bar stays pinned at
        // the bottom). Every other kind gets a single outer scroll wrapper so long lists
        // don't get clipped when the panel shrinks.
        let body = if matches!(self.kind, Kind::AiChat) {
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
        let focused = self.focus_handle.is_focused(window);
        let border_color = if focused {
            cx.theme().ring
        } else {
            gpui::transparent_black()
        };
        // track_focus makes the panel area itself focusable, so clicks inside the body land
        // focus on `focus_handle` and trigger our on_focus_in subscription.
        div()
            .track_focus(&self.focus_handle)
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

fn sample_candles() -> Vec<Candle> {
    // 30 deterministic dummy candles, gently trending up.
    let mut v = Vec::with_capacity(30);
    let mut close = 180.0;
    for i in 0..30 {
        let drift = ((i as f64 * 0.7).sin() * 4.0) + (i as f64 * 0.4);
        let open = close + ((i as f64 * 1.3).cos() * 1.5);
        close = open + drift;
        let high = open.max(close) + 1.5 + (i as f64 * 0.13).abs() % 2.0;
        let low = open.min(close) - 1.0 - (i as f64 * 0.17).abs() % 2.0;
        v.push(Candle {
            date: SharedString::from(format!("D{:02}", i + 1)),
            open,
            high,
            low,
            close,
        });
    }
    v
}

fn render_chart(_window: &mut Window, cx: &mut Context<ContentPanel>) -> impl IntoElement {
    let theme = cx.theme();
    let candles = sample_candles();
    let line_data: Vec<Candle> = candles.clone();

    v_flex()
        .w_full()
        .p_3()
        .gap_2()
        .child(
            h_flex()
                .gap_3()
                .items_baseline()
                .child(div().text_lg().font_semibold().child("AAPL"))
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .text_sm()
                        .child("Apple Inc. · NASDAQ"),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_color(theme.chart_bullish)
                        .font_semibold()
                        .child(format!("${:.2}", candles.last().map(|c| c.close).unwrap_or(0.))),
                ),
        )
        .child(
            div()
                .h(gpui::px(220.))
                .w_full()
                .child(
                    CandlestickChart::new(candles)
                        .x(|c: &Candle| c.date.clone())
                        .open(|c: &Candle| c.open)
                        .close(|c: &Candle| c.close)
                        .high(|c: &Candle| c.high)
                        .low(|c: &Candle| c.low),
                ),
        )
        .child(
            div()
                .h(gpui::px(120.))
                .w_full()
                .child(
                    LineChart::new(line_data)
                        .x(|c: &Candle| c.date.clone())
                        .y(|c: &Candle| c.close)
                        .stroke(theme.chart_1)
                        .natural(),
                ),
        )
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
}

const NEWS: &[NewsItem] = &[
    NewsItem { time: "10:32", tag: "AAPL",  headline: "Apple reports Q2 earnings beat, services revenue up 14%", source: "Reuters" },
    NewsItem { time: "10:15", tag: "FED",   headline: "Powell signals patient approach on rate cuts",            source: "Bloomberg" },
    NewsItem { time: "09:58", tag: "NVDA",  headline: "NVIDIA hits new all-time high on AI chip demand",         source: "CNBC" },
    NewsItem { time: "09:42", tag: "META",  headline: "Meta unveils next-gen Ray-Ban smart glasses",             source: "The Verge" },
    NewsItem { time: "09:30", tag: "MKT",   headline: "S&P 500 opens flat as traders await CPI data",            source: "WSJ" },
    NewsItem { time: "09:14", tag: "TSLA",  headline: "Tesla deliveries miss estimates; stock down 2% premkt",   source: "Reuters" },
    NewsItem { time: "08:55", tag: "BTC",   headline: "Bitcoin steady at $68k as ETF inflows resume",            source: "CoinDesk" },
    NewsItem { time: "08:30", tag: "ECON",  headline: "Initial jobless claims fall to 218k, below estimates",    source: "BLS" },
    NewsItem { time: "08:12", tag: "OIL",   headline: "Brent crude rises 1.2% on Middle East supply concerns",   source: "Bloomberg" },
    NewsItem { time: "07:45", tag: "GOOGL", headline: "Alphabet announces $70B share buyback plan",              source: "FT" },
];

fn render_news(_window: &mut Window, cx: &mut Context<ContentPanel>) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .w_full()
        .p_2()
        .gap_1()
        .children(NEWS.iter().map(|n| {
            v_flex()
                .px_2()
                .py_2()
                .gap_1()
                .border_b_1()
                .border_color(theme.border)
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
        }))
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

// Convenience re-export for callers that still use the old `PanelKind` name.
pub type PanelKind = Kind;
pub const PANEL_KINDS: &[Kind] = Kind::ALL;

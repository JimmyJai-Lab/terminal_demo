use std::time::Duration;

use chrono::{DateTime, Local};
use gpui::{
    Action, Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Task,
    Window, actions, div, px,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _, StyledExt as _, Theme, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogButtonProps, DialogFooter, DialogHeader, DialogTitle},
    h_flex,
    menu::DropdownMenu as _,
    notification::Notification,
    separator::Separator,
    v_flex,
};
use serde::Deserialize;

const FONT_SIZE_MIN: f32 = 10.0;
const FONT_SIZE_MAX: f32 = 28.0;
const FONT_SIZE_DEFAULT: f32 = 16.0;

fn adjust_font_size(delta: f32, window: &mut Window, cx: &mut gpui::App) {
    let current: f32 = cx.global::<Theme>().font_size.into();
    let next = (current + delta).clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
    if (next - current).abs() < 0.01 {
        return;
    }
    cx.global_mut::<Theme>().font_size = px(next);
    window.refresh();
}

fn set_font_size(value: f32, window: &mut Window, cx: &mut gpui::App) {
    cx.global_mut::<Theme>().font_size = px(value.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX));
    window.refresh();
}

use crate::panels::PANEL_KINDS;

actions!(terminal_demo, [ResetLayout]);

#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = terminal_demo, no_json)]
pub struct AddPanel(pub SharedString);

pub struct TopBar {
    title: SharedString,
    now: DateTime<Local>,
    _tick_task: Task<()>,
}

impl TopBar {
    pub fn new(
        title: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Tick once per second so the seconds field stays accurate. The cost is one re-render
        // of the top bar per second; the rest of the workspace is unaffected.
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

        Self {
            title: title.into(),
            now: Local::now(),
            _tick_task,
        }
    }
}

impl Render for TopBar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = _cx.theme();

        let add_menu = Button::new("add-panel")
            .label("+ Panel")
            .small()
            .ghost()
            .dropdown_menu(|menu, _, _| {
                let mut menu = menu;
                for kind in PANEL_KINDS {
                    menu = menu.menu(
                        kind.display(),
                        Box::new(AddPanel(SharedString::from(kind.id()))),
                    );
                }
                menu
            });

        let notify_btn = Button::new("notify")
            .label("Notify")
            .small()
            .ghost()
            .on_click(|_, window, cx| {
                window.push_notification(
                    Notification::success("Order filled · BUY 100 NVDA @ $873.50")
                        .title("Trade executed"),
                    cx,
                );
            });

        let settings_btn = Button::new("settings")
            .label("⚙")
            .small()
            .ghost()
            .on_click(|_, window, cx| open_settings_dialog(window, cx));

        // %a = Mon, %b = May, %d = 06; %H:%M:%S 24h; %:z = +08:00
        let date_str = SharedString::from(self.now.format("%a, %b %d %Y").to_string());
        let time_str = SharedString::from(self.now.format("%H:%M:%S").to_string());
        let tz_str = SharedString::from(format!("UTC{}", self.now.format("%:z")));

        let clock = div()
            .flex()
            .flex_row()
            .items_baseline()
            .gap_2()
            .child(div().text_sm().font_semibold().child(time_str))
            .child(div().text_xs().text_color(theme.muted_foreground).child(date_str))
            .child(div().text_xs().text_color(theme.muted_foreground).child(tz_str));

        div()
            .h(px(36.))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .px_3()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.tab_bar)
            .child(
                div()
                    .text_sm()
                    .text_color(theme.foreground)
                    .child(self.title.clone()),
            )
            .child(div().flex_1())
            .child(clock)
            .child(div().w(px(8.)))
            .child(notify_btn)
            .child(add_menu)
            .child(settings_btn)
    }
}

fn open_settings_dialog(window: &mut Window, cx: &mut gpui::App) {
    window.open_dialog(cx, |dialog, _, _| {
        dialog
            .max_w(px(420.))
            .button_props(DialogButtonProps::default().ok_text("Done"))
            .child(
                v_flex()
                    .gap_4()
                    .child(
                        DialogHeader::new()
                            .px_4()
                            .pt_4()
                            .child(DialogTitle::new().child("Settings")),
                    )
                    .child(render_font_size_setting())
                    .child(div().px_4().child(Separator::horizontal()))
                    .child(LayoutSetting)
                    .child(DialogFooter::new().px_4().pb_2()),
            )
    });
}

#[derive(gpui::IntoElement)]
struct LayoutSetting;

impl gpui::RenderOnce for LayoutSetting {
    fn render(self, _window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .px_4()
            .gap_2()
            .child(div().text_sm().font_semibold().child("Layout"))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("reset-layout")
                            .label("Reset Layout")
                            .small()
                            .outline()
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(ResetLayout), cx);
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("Restores the default 3-pane workspace."),
                    ),
            )
    }
}

fn render_font_size_setting() -> impl IntoElement {
    v_flex()
        .px_4()
        .gap_2()
        .child(div().text_sm().font_semibold().child("Font size"))
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    Button::new("font-minus")
                        .label("−")
                        .small()
                        .outline()
                        .on_click(|_, window, cx| adjust_font_size(-1.0, window, cx)),
                )
                .child(FontSizeReadout)
                .child(
                    Button::new("font-plus")
                        .label("+")
                        .small()
                        .outline()
                        .on_click(|_, window, cx| adjust_font_size(1.0, window, cx)),
                )
                .child(div().w(px(12.)))
                .child(
                    Button::new("font-reset")
                        .label("Reset")
                        .small()
                        .ghost()
                        .on_click(|_, window, cx| set_font_size(FONT_SIZE_DEFAULT, window, cx)),
                ),
        )
}

#[derive(gpui::IntoElement)]
struct FontSizeReadout;

impl gpui::RenderOnce for FontSizeReadout {
    fn render(self, _window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let size: f32 = cx.global::<Theme>().font_size.into();
        div()
            .min_w(px(56.))
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(SharedString::from(format!("{size:.0} px")))
    }
}

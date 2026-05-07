use std::time::Duration;

use chrono::{DateTime, Local};
use crate::persistence::SavedLayouts;
use gpui::{
    Action, Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Task,
    Window, actions, div, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _, Theme, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogButtonProps, DialogFooter, DialogHeader, DialogTitle},
    h_flex,
    menu::DropdownMenu as _,
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
    apply_font_size(next, window, cx);
}

fn set_font_size(value: f32, window: &mut Window, cx: &mut gpui::App) {
    apply_font_size(value.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX), window, cx);
}

fn apply_font_size(value: f32, window: &mut Window, cx: &mut gpui::App) {
    cx.global_mut::<Theme>().font_size = px(value);
    window.refresh();
    if let Err(err) = crate::persistence::save_font_size(value) {
        log::warn!("save font size failed: {err:?}");
    }
}

use crate::panels::PANEL_KINDS;

actions!(
    terminal_demo,
    [
        ResetLayout,
        ToggleAiChat,
        ToggleTrading,
        SaveLayout,
        SaveLayoutCurrent,
        ManageLayouts
    ]
);

#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = terminal_demo, no_json)]
pub struct AddPanel(pub SharedString);

#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = terminal_demo, no_json)]
pub struct AskAi(pub SharedString);

#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = terminal_demo, no_json)]
pub struct ApplyLayout(pub SharedString);

#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = terminal_demo, no_json)]
pub struct DeleteLayout(pub SharedString);


pub struct TopBar {
    title: SharedString,
    now: DateTime<Local>,
    /// Display name + writability for the currently active layout, shown on
    /// the layout button. The host (TerminalWorkspace) pushes updates via
    /// `set_current_layout` whenever a preset/saved layout is applied.
    current_layout_label: SharedString,
    /// True when the active layout can be overwritten in place (i.e. it's a
    /// user-saved layout, not a preset). Drives the Save button: when true,
    /// Save overwrites; otherwise it falls through to Save-As.
    current_layout_overwritable: bool,
    /// Cached saved-layouts list used to render the Layout dropdown's "Saved"
    /// section. Populated on construction and refreshed by the workspace
    /// whenever layouts change — avoids hitting localStorage / disk on every
    /// TopBar render.
    saved_layouts: SavedLayouts,
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
            current_layout_label: SharedString::from("Layout"),
            current_layout_overwritable: false,
            saved_layouts: crate::persistence::load_layouts(),
            _tick_task,
        }
    }

    /// Update the layout button's label and Save semantics. Called by the
    /// workspace whenever the active layout changes (on apply / save-as / etc).
    pub fn set_current_layout(
        &mut self,
        label: impl Into<SharedString>,
        overwritable: bool,
        cx: &mut Context<Self>,
    ) {
        self.current_layout_label = label.into();
        self.current_layout_overwritable = overwritable;
        cx.notify();
    }

    /// Re-read saved layouts from persistence. Workspace calls this after
    /// every save/delete so the dropdown stays in sync without paying a disk
    /// read per render.
    pub fn refresh_saved_layouts(&mut self, cx: &mut Context<Self>) {
        self.saved_layouts = crate::persistence::load_layouts();
        cx.notify();
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
                // Singleton kinds (AI Chat, Position, Execution) are reserved for
                // toolbar toggles only — hide them here so the user can't dock
                // duplicates from this menu.
                for kind in PANEL_KINDS.iter().filter(|k| !k.is_singleton()) {
                    menu = menu.menu(
                        kind.display(),
                        Box::new(AddPanel(SharedString::from(kind.id()))),
                    );
                }
                menu
            });

        let saved_names: Vec<SharedString> = self
            .saved_layouts
            .keys()
            .map(|name| SharedString::from(name.clone()))
            .collect();
        let layout_menu = Button::new("layout")
            .label(self.current_layout_label.clone())
            .small()
            .ghost()
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.label("Predefined");
                for preset in PRESET_LAYOUTS {
                    menu = menu.menu(
                        *preset.1,
                        Box::new(ApplyLayout(SharedString::from(preset.0))),
                    );
                }
                menu = menu
                    .separator()
                    .menu("Save current layout…", Box::new(SaveLayout))
                    .menu("Manage layouts…", Box::new(ManageLayouts));

                if !saved_names.is_empty() {
                    menu = menu.separator().label("Saved");
                    for name in &saved_names {
                        menu = menu.menu(
                            name.clone(),
                            Box::new(ApplyLayout(name.clone())),
                        );
                    }
                }
                menu
            });

        let save_layout_btn = {
            let overwritable = self.current_layout_overwritable;
            let (label, tooltip) = if overwritable {
                ("Save", "Save changes to current layout")
            } else {
                ("Save As", "Save current layout under a new name (presets are read-only)")
            };
            Button::new("save-layout")
                .label(label)
                .small()
                .ghost()
                .tooltip(tooltip)
                .on_click(move |_, window, cx| {
                    if overwritable {
                        window.dispatch_action(Box::new(SaveLayoutCurrent), cx);
                    } else {
                        window.dispatch_action(Box::new(SaveLayout), cx);
                    }
                })
        };

        let trading_btn = Button::new("trading")
            .icon(IconName::SquareTerminal)
            .small()
            .ghost()
            .tooltip("Toggle trading panels")
            .on_click(|_, window, cx| {
                window.dispatch_action(Box::new(ToggleTrading), cx);
            });

        let ai_chat_btn = Button::new("ai-chat")
            .icon(IconName::Bot)
            .small()
            .ghost()
            .tooltip("Toggle AI chat panel")
            .on_click(|_, window, cx| {
                window.dispatch_action(Box::new(ToggleAiChat), cx);
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
            .child(add_menu)
            .child(layout_menu)
            .child(save_layout_btn)
            .child(trading_btn)
            .child(ai_chat_btn)
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

// Preset layout names. The id strings are passed through `ApplyLayout`; the
// workspace dispatches to a builder when the id starts with `__preset_`,
// otherwise it loads from saved-layouts persistence.
pub const PRESET_GENERAL: &str = "__preset_general";
pub const PRESET_FUNDAMENTAL: &str = "__preset_fundamental";
pub const PRESET_TECHNICAL: &str = "__preset_technical";

const PRESET_LAYOUTS: &[(&str, &&str)] = &[
    (PRESET_GENERAL, &"General"),
    (PRESET_FUNDAMENTAL, &"Fundamental"),
    (PRESET_TECHNICAL, &"Technical"),
];

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

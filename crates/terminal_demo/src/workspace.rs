use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, AppContext as _, Axis, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Task, Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, Placement, Root, Sizable as _, WindowExt as _,
    dock::{
        DockArea, DockAreaState, DockEvent, DockItem, DockPlacement, PanelInfo, PanelState,
        PanelView,
    },
};

use crate::bottom_bar::BottomBar;
use crate::panels::{self, ContentPanel, Kind, LastFocusedTabPanel};
use crate::persistence;
use crate::top_bar::{
    AddPanel, ApplyLayout, AskAi, DeleteLayout, ManageLayouts, PRESET_FUNDAMENTAL, PRESET_GENERAL,
    PRESET_TECHNICAL, ResetLayout, SaveLayout, SaveLayoutCurrent, ToggleAiChat, ToggleTrading,
    TopBar,
};

const LAYOUT_VERSION: usize = 1;
const DOCK_AREA_ID: &str = "main-dock";

pub struct TerminalWorkspace {
    top_bar: Entity<TopBar>,
    bottom_bar: Entity<BottomBar>,
    dock_area: Entity<DockArea>,
    last_saved: Option<DockAreaState>,
    _save_task: Option<Task<()>>,
    /// Action dispatch in gpui walks up from the focused element. On a fresh
    /// app start, nothing inside the workspace has focus, so toolbar-button
    /// `dispatch_action` calls would never reach our `on_action` handlers
    /// (the dispatch path is just `[root]`). We keep a focus handle on the
    /// workspace's outer div and focus it on first render so the dispatch
    /// path always includes the workspace.
    focus_handle: FocusHandle,
    focused_once: bool,
    /// Which named layout (if any) is currently active. Drives the layout
    /// button label and decides whether the Save button overwrites in place
    /// (Saved) or pops the Save-As dialog (Predefined / Unnamed — presets
    /// are read-only).
    current_layout: persistence::CurrentLayoutKind,
    /// Singleton panels — only one of each may exist at a time. The toolbar
    /// toggles, the +Panel menu, and on_removed all consult these slots.
    ai_chat_panel: Option<Entity<ContentPanel>>,
    position_panel: Option<Entity<ContentPanel>>,
    execution_panel: Option<Entity<ContentPanel>>,
}

impl TerminalWorkspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let dock_area =
            cx.new(|cx| DockArea::new(DOCK_AREA_ID, Some(LAYOUT_VERSION), window, cx));
        let weak_dock = dock_area.downgrade();

        if !try_load(&dock_area, window, cx) {
            apply_default_layout(weak_dock.clone(), window, cx);
        }

        cx.subscribe_in(
            &dock_area,
            window,
            |this, dock_area, ev: &DockEvent, window, cx| {
                if matches!(ev, DockEvent::LayoutChanged) {
                    this.schedule_save(dock_area.clone(), window, cx);
                }
            },
        )
        .detach();

        let top_bar = cx.new(|cx| TopBar::new("terminal_demo", window, cx));
        let bottom_bar = cx.new(|cx| BottomBar::new(window, cx));

        let mut this = Self {
            top_bar,
            bottom_bar,
            dock_area,
            last_saved: None,
            _save_task: None,
            focus_handle: cx.focus_handle(),
            focused_once: false,
            current_layout: persistence::load_current_layout(),
            ai_chat_panel: None,
            position_panel: None,
            execution_panel: None,
        };
        // The persisted layout may already include singleton panels; adopt them
        // so the toolbar toggles act on the existing instances.
        this.adopt_existing_singletons(window, cx);
        this.push_layout_label_to_top_bar(cx);
        this
    }

    fn schedule_save(
        &mut self,
        dock_area: Entity<DockArea>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._save_task = Some(cx.spawn_in(window, async move |this, window| {
            window
                .background_executor()
                .timer(Duration::from_millis(500))
                .await;
            _ = this.update_in(window, |this, _, cx| {
                let state = dock_state_without_singletons(dock_area.read(cx).dump(cx));
                if Some(&state) == this.last_saved.as_ref() {
                    return;
                }
                if let Err(err) = persistence::save(&state) {
                    log::warn!("save layout failed: {err:?}");
                } else {
                    this.last_saved = Some(state);
                }
            });
        }));
    }

    fn on_add_panel(
        &mut self,
        action: &AddPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(kind) = Kind::from_id(action.0.as_ref()) else {
            return;
        };

        // Singleton kinds: if one is already alive, focus its parent tab instead
        // of spawning a duplicate.
        if kind.is_singleton() {
            self.refresh_singleton_slots(cx);
            if let Some(existing) = self.singleton_slot(kind).clone() {
                if let Some(tp) = existing
                    .read(cx)
                    .parent_tab_panel()
                    .and_then(|w| w.upgrade())
                {
                    let arc: Arc<dyn PanelView> = Arc::new(existing);
                    if let Some(ix) = tp.read(cx).index_of_panel(&arc) {
                        tp.update(cx, |tp, cx| tp.set_active_ix(ix, window, cx));
                    }
                }
                return;
            }
        }

        let panel = panels::build_kind(kind, window, cx);

        // Prefer the most recently focused TabPanel so the new tab lands where the user is
        // working. Falls back to DockArea::add_panel(Center) if nothing was ever focused
        // (e.g. fresh layout, or the focused panel was just closed).
        //
        // `mark_focused` skips singleton (pinned) TabPanels, so the value here
        // is guaranteed to be a non-pinned TabPanel — no extra filtering needed.
        let target = cx
            .global::<LastFocusedTabPanel>()
            .0
            .borrow()
            .clone()
            .and_then(|w| w.upgrade());

        if let Some(tab_panel) = target {
            tab_panel.update(cx, |tp, cx| tp.add_panel(panel.clone(), window, cx));
        } else {
            self.dock_area.update(cx, |dock_area, cx| {
                dock_area.add_panel(panel.clone(), DockPlacement::Center, None, window, cx);
            });
        }

        // Track the new singleton so the next click toggles/focuses the same one.
        if kind.is_singleton() {
            if let Ok(entity) = panel.view().downcast::<ContentPanel>() {
                *self.singleton_slot(kind) = Some(entity);
            }
        }
    }

    /// Drop singleton slots whose underlying panel is no longer attached to a
    /// TabPanel (e.g. the user closed the tab). ContentPanel::on_removed clears
    /// `parent_tab_panel`, so we can use that as the staleness signal.
    fn refresh_singleton_slots(&mut self, cx: &App) {
        for slot in [
            &mut self.ai_chat_panel,
            &mut self.position_panel,
            &mut self.execution_panel,
        ] {
            if let Some(entity) = slot {
                if entity.read(cx).parent_tab_panel().is_none() {
                    *slot = None;
                }
            }
        }
    }

    /// Persist the current-layout pointer and refresh the toolbar button.
    fn set_current_layout(
        &mut self,
        layout: persistence::CurrentLayoutKind,
        cx: &mut Context<Self>,
    ) {
        if self.current_layout == layout {
            return;
        }
        if let Err(err) = persistence::save_current_layout(&layout) {
            log::warn!("save current layout failed: {err:?}");
        }
        self.current_layout = layout;
        self.push_layout_label_to_top_bar(cx);
    }

    fn push_layout_label_to_top_bar(&self, cx: &mut Context<Self>) {
        let (label, overwritable) = match &self.current_layout {
            persistence::CurrentLayoutKind::Unnamed => ("Layout".to_string(), false),
            persistence::CurrentLayoutKind::Predefined { id } => (preset_display(id), false),
            persistence::CurrentLayoutKind::Saved { name } => (name.clone(), true),
        };
        self.top_bar.update(cx, |top_bar, cx| {
            top_bar.set_current_layout(label, overwritable, cx);
        });
    }

    fn refresh_top_bar_saved_layouts(&self, cx: &mut Context<Self>) {
        self.top_bar
            .update(cx, |top_bar, cx| top_bar.refresh_saved_layouts(cx));
    }

    /// Re-focus the workspace's outer div. Called whenever we replace the dock
    /// center or remove panels, so that the previously focused element (which
    /// may now be detached) doesn't leave the window in an unfocused state —
    /// gpui's `dispatch_action` needs *some* focused element below the root for
    /// our `on_action` handlers to be reachable, otherwise toolbar buttons
    /// stop working until the user clicks back into a panel.
    fn reclaim_focus(&self, window: &mut Window, cx: &mut App) {
        self.focus_handle.clone().focus(window, cx);
    }

    fn singleton_slot(&mut self, kind: Kind) -> &mut Option<Entity<ContentPanel>> {
        match kind {
            Kind::AiChat => &mut self.ai_chat_panel,
            Kind::Position => &mut self.position_panel,
            Kind::Execution => &mut self.execution_panel,
            _ => unreachable!("singleton_slot called for non-singleton {:?}", kind),
        }
    }

    /// After a layout is loaded from disk (or restored), the singleton slots
    /// are empty even when the layout itself contains AI Chat / Position /
    /// Execution panels. Walk the dock tree once and adopt them so the toolbar
    /// toggles operate on the existing instances instead of spawning
    /// duplicates. Also pin each adopted singleton's parent TabPanel so users
    /// can't drag other tabs into it (older saved layouts predate the pinning
    /// helper, so we fix them up here).
    fn adopt_existing_singletons(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Reset first — anything not found in the live tree is stale.
        self.ai_chat_panel = None;
        self.position_panel = None;
        self.execution_panel = None;
        let center = self.dock_area.read(cx).center().clone();
        adopt_from_item(&center, cx, self);

        // `Panel::on_added_to` runs synchronously inside `TabPanel::add_panel`,
        // so by the time we get here every adopted singleton has its
        // `parent_tab_panel` populated. Pin those parents directly.
        let mut pin = |slot: &Option<Entity<ContentPanel>>| {
            if let Some(entity) = slot {
                if let Some(tp) = entity
                    .read(cx)
                    .parent_tab_panel()
                    .and_then(|w| w.upgrade())
                {
                    tp.update(cx, |tp, cx| tp.set_pinned(true, window, cx));
                }
            }
        };
        pin(&self.ai_chat_panel);
        pin(&self.position_panel);
        pin(&self.execution_panel);
    }

    fn on_reset_layout(
        &mut self,
        _: &ResetLayout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = persistence::clear();
        let weak = self.dock_area.downgrade();
        apply_default_layout(weak, window, cx);
        self.last_saved = None;
        self.adopt_existing_singletons(window, cx);
        self.reclaim_focus(window, cx);
        // Reset goes back to the General preset by definition.
        self.set_current_layout(
            persistence::CurrentLayoutKind::Predefined {
                id: PRESET_GENERAL.to_string(),
            },
            cx,
        );
    }

    fn on_toggle_ai_chat(
        &mut self,
        _: &ToggleAiChat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_singleton_slots(cx);

        // If the panel is currently open, find its parent TabPanel and remove it.
        if let Some(panel) = self.ai_chat_panel.take() {
            let parent = panel.read(cx).parent_tab_panel();
            if let Some(tp) = parent.and_then(|w| w.upgrade()) {
                let arc: Arc<dyn PanelView> = Arc::new(panel);
                tp.update(cx, |tp, cx| tp.remove_panel(arc, window, cx));
                self.reclaim_focus(window, cx);
                return;
            }
            // Stale entity (panel was closed elsewhere) — fall through to open a new one.
        }

        self.open_ai_chat_panel(window, cx);
    }

    fn on_ask_ai(&mut self, action: &AskAi, window: &mut Window, cx: &mut Context<Self>) {
        // Defer the work so we don't restructure the dock tree in the middle of
        // the click handler that came from a button inside the dock tree itself
        // — that previously hung the event loop.
        let prompt = action.0.clone();
        let weak_self = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            let _ = weak_self.update(cx, |this, cx| {
                this.refresh_singleton_slots(cx);
                if this.ai_chat_panel.is_none() {
                    this.open_ai_chat_panel(window, cx);
                }
                let Some(panel) = this.ai_chat_panel.clone() else {
                    return;
                };

                if let Some(tp) = panel
                    .read(cx)
                    .parent_tab_panel()
                    .and_then(|w| w.upgrade())
                {
                    let arc: Arc<dyn PanelView> = Arc::new(panel.clone());
                    if let Some(ix) = tp.read(cx).index_of_panel(&arc) {
                        tp.update(cx, |tp, cx| tp.set_active_ix(ix, window, cx));
                    }
                }

                let input = panel.read(cx).chat_input().cloned();
                if let Some(input) = input {
                    let handle = input.read(cx).focus_handle(cx);
                    input.update(cx, |state, cx| {
                        state.set_value(prompt, window, cx);
                    });
                    handle.focus(window, cx);
                }
            });
        });
    }

    fn open_ai_chat_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.new(|cx| ContentPanel::new(Kind::AiChat, window, cx));
        let arc: Arc<dyn PanelView> = Arc::new(entity.clone());
        let viewport_w: f32 = window.viewport_size().width.into();
        let target_width = px((viewport_w / 4.0).max(200.0));
        self.dock_area.update(cx, |dock, cx| {
            dock.dock_to_window_edge(arc, Placement::Right, Some(target_width), window, cx);
        });
        pin_panel_tab(&entity, window, cx);
        self.ai_chat_panel = Some(entity);
    }

    fn on_toggle_trading(
        &mut self,
        _: &ToggleTrading,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_singleton_slots(cx);

        // If either trading panel is open, the toggle closes both.
        let close_position = self.position_panel.take();
        let close_execution = self.execution_panel.take();
        if close_position.is_some() || close_execution.is_some() {
            for entity in [close_position, close_execution].into_iter().flatten() {
                if let Some(tp) = entity
                    .read(cx)
                    .parent_tab_panel()
                    .and_then(|w| w.upgrade())
                {
                    let arc: Arc<dyn PanelView> = Arc::new(entity);
                    tp.update(cx, |tp, cx| tp.remove_panel(arc, window, cx));
                }
            }
            self.reclaim_focus(window, cx);
            return;
        }

        // Open both as a paired bottom row: Position spans the bottom, Execution
        // takes 1/4 of that row on the right. The bottom row itself is sized at
        // 1/5 of the current viewport height.
        let viewport = window.viewport_size();
        let viewport_w: f32 = viewport.width.into();
        let viewport_h: f32 = viewport.height.into();
        let bottom_height = px((viewport_h / 5.0).max(120.0));
        let exec_width = px((viewport_w / 4.0).max(180.0));

        let position = cx.new(|cx| ContentPanel::new(Kind::Position, window, cx));
        let execution = cx.new(|cx| ContentPanel::new(Kind::Execution, window, cx));
        let position_arc: Arc<dyn PanelView> = Arc::new(position.clone());
        let execution_arc: Arc<dyn PanelView> = Arc::new(execution.clone());

        let weak = self.dock_area.downgrade();
        self.dock_area.update(cx, |dock, cx| {
            // Bottom row: SplitH of [Position | Execution] with Execution sized to 1/4.
            let bottom_row = DockItem::split_with_sizes(
                Axis::Horizontal,
                vec![
                    DockItem::tabs(vec![position_arc], &weak, window, cx),
                    DockItem::tabs(vec![execution_arc], &weak, window, cx),
                ],
                vec![None, Some(exec_width)],
                &weak,
                window,
                cx,
            );

            let existing = dock.center().clone();
            let new_center = DockItem::split_with_sizes(
                Axis::Vertical,
                vec![existing, bottom_row],
                vec![None, Some(bottom_height)],
                &weak,
                window,
                cx,
            );
            dock.set_center(new_center, window, cx);
        });

        pin_panel_tab(&position, window, cx);
        pin_panel_tab(&execution, window, cx);
        self.position_panel = Some(position);
        self.execution_panel = Some(execution);
    }

    fn on_apply_layout(
        &mut self,
        action: &ApplyLayout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = action.0.clone();
        let weak_dock = self.dock_area.downgrade();

        // Predefined preset?
        let preset_builder: Option<fn(&gpui::WeakEntity<DockArea>, &mut Window, &mut App) -> DockItem> =
            match id.as_ref() {
                PRESET_GENERAL => Some(build_general_layout),
                PRESET_FUNDAMENTAL => Some(build_fundamental_layout),
                PRESET_TECHNICAL => Some(build_technical_layout),
                _ => None,
            };
        if let Some(builder) = preset_builder {
            self.dock_area.update(cx, |dock, cx| {
                dock.set_version(LAYOUT_VERSION, window, cx);
                let item = builder(&weak_dock, window, cx);
                dock.set_center(item, window, cx);
            });
            self.adopt_existing_singletons(window, cx);
            self.reclaim_focus(window, cx);
            self.set_current_layout(
                persistence::CurrentLayoutKind::Predefined { id: id.to_string() },
                cx,
            );
            return;
        }

        // Saved user layout — load from persistence and apply.
        let layouts = persistence::load_layouts();
        let Some(state) = layouts.get(id.as_ref()).cloned() else {
            notify_warning(
                window,
                cx,
                "Layout missing",
                &format!("No saved layout named '{id}'"),
            );
            return;
        };
        let _ = self
            .dock_area
            .update(cx, |dock, cx| dock.load(state, window, cx));
        self.adopt_existing_singletons(window, cx);
        self.reclaim_focus(window, cx);
        self.set_current_layout(
            persistence::CurrentLayoutKind::Saved {
                name: id.to_string(),
            },
            cx,
        );
    }

    fn on_save_layout(&mut self, _: &SaveLayout, window: &mut Window, cx: &mut Context<Self>) {
        let dock_area = self.dock_area.clone();
        let weak_self = cx.entity().downgrade();
        let name_state = cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx).placeholder("Layout name")
        });
        let name_handle = name_state.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let saving_state = name_handle.clone();
            let dock_area = dock_area.clone();
            let weak_self = weak_self.clone();
            dialog
                .max_w(px(420.))
                .button_props(
                    gpui_component::dialog::DialogButtonProps::default()
                        .show_cancel(true)
                        .ok_text("Save")
                        .on_ok(move |_, window, cx| {
                            let name = saving_state.read(cx).value().to_string();
                            let trimmed = name.trim();
                            if trimmed.is_empty() {
                                notify_warning(window, cx, "Save layout", "Enter a name first");
                                return false;
                            }
                            let state = dock_state_without_singletons(dock_area.read(cx).dump(cx));
                            match persistence::upsert_layout(trimmed, state) {
                                Ok(()) => {
                                    notify_success(
                                        window,
                                        cx,
                                        "Layout saved",
                                        format!("Saved '{trimmed}'"),
                                    );
                                    let trimmed = trimmed.to_string();
                                    _ = weak_self.update(cx, |this, cx| {
                                        this.set_current_layout(
                                            persistence::CurrentLayoutKind::Saved {
                                                name: trimmed,
                                            },
                                            cx,
                                        );
                                        this.refresh_top_bar_saved_layouts(cx);
                                    });
                                    true
                                }
                                Err(err) => {
                                    log::warn!("save layout failed: {err:?}");
                                    notify_error(
                                        window,
                                        cx,
                                        "Save layout",
                                        "Failed to save layout",
                                    );
                                    false
                                }
                            }
                        }),
                )
                .child(
                    gpui::div()
                        .px_4()
                        .pt_4()
                        .pb_2()
                        .child(SharedString::from("Save current layout as:")),
                )
                .child(
                    gpui::div().px_4().pb_4().child(
                        gpui_component::input::Input::new(&name_handle).small(),
                    ),
                )
        });
    }

    fn on_save_layout_current(
        &mut self,
        _: &SaveLayoutCurrent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Only Saved(name) is overwritable. Anything else falls back to the
        // Save-As dialog so presets stay immutable.
        let persistence::CurrentLayoutKind::Saved { name } = self.current_layout.clone() else {
            self.on_save_layout(&SaveLayout, window, cx);
            return;
        };
        let state = dock_state_without_singletons(self.dock_area.read(cx).dump(cx));
        match persistence::upsert_layout(&name, state) {
            Ok(()) => {
                notify_success(window, cx, "Layout saved", format!("Saved '{name}'"));
                self.refresh_top_bar_saved_layouts(cx);
            }
            Err(err) => {
                log::warn!("save layout failed: {err:?}");
                notify_error(window, cx, "Save layout", "Failed to save layout");
            }
        }
    }

    fn on_manage_layouts(
        &mut self,
        _: &ManageLayouts,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let layouts = persistence::load_layouts();
        if layouts.is_empty() {
            notify_info(
                window,
                cx,
                "No saved layouts",
                "Use 'Save current layout…' to create one first.",
            );
            return;
        }

        window.open_dialog(cx, move |dialog, _, cx| {
            let layouts = layouts.clone();
            let muted = cx.theme().muted_foreground;
            let border = cx.theme().border;
            let mut body = gpui::div().px_4().pb_4().pt_2().flex().flex_col().gap_2();
            body = body.child(
                gpui::div()
                    .text_xs()
                    .text_color(muted)
                    .child("Click a layout to apply it, or × to delete."),
            );
            for name in layouts.keys() {
                let n_apply = SharedString::from(name.clone());
                let n_delete = SharedString::from(name.clone());
                body = body.child(
                    gpui::div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(border)
                        .rounded(px(4.))
                        .child(
                            gpui::div()
                                .flex_1()
                                .text_sm()
                                .child(SharedString::from(name.clone())),
                        )
                        .child(
                            gpui_component::button::Button::new(SharedString::from(format!(
                                "apply-{name}"
                            )))
                            .label("Apply")
                            .small()
                            .on_click(move |_, window, cx| {
                                window.dispatch_action(
                                    Box::new(ApplyLayout(n_apply.clone())),
                                    cx,
                                );
                                window.close_all_dialogs(cx);
                            }),
                        )
                        .child(
                            gpui_component::button::Button::new(SharedString::from(format!(
                                "delete-{name}"
                            )))
                            .label("×")
                            .small()
                            .on_click(move |_, window, cx| {
                                window.dispatch_action(
                                    Box::new(DeleteLayout(n_delete.clone())),
                                    cx,
                                );
                                window.close_all_dialogs(cx);
                            }),
                        ),
                );
            }
            dialog
                .title(SharedString::from("Saved Layouts"))
                .max_w(px(480.))
                .button_props(
                    gpui_component::dialog::DialogButtonProps::default().ok_text("Done"),
                )
                .child(body)
        });
    }

    fn on_delete_layout(
        &mut self,
        action: &DeleteLayout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = action.0.clone();
        match persistence::delete_layout(name.as_ref()) {
            Ok(()) => {
                notify_success(window, cx, "Layout deleted", format!("Deleted '{name}'"));
                // If the deleted layout was the active one, clear the pointer
                // so the toolbar stops showing its name and Save reverts to
                // Save-As.
                if matches!(
                    &self.current_layout,
                    persistence::CurrentLayoutKind::Saved { name: cur } if cur == name.as_ref()
                ) {
                    self.set_current_layout(persistence::CurrentLayoutKind::Unnamed, cx);
                }
                self.refresh_top_bar_saved_layouts(cx);
            }
            Err(err) => {
                log::warn!("delete layout failed: {err:?}");
                notify_error(window, cx, "Delete layout", "Failed to delete layout");
            }
        }
    }
}

impl Render for TerminalWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (bg, fg) = {
            let theme = cx.theme();
            (theme.background, theme.foreground)
        };
        let sheet_layer = Root::render_sheet_layer(window, cx);
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        // First render: claim focus so toolbar `dispatch_action` calls have a
        // dispatch path that includes our `on_action` handlers below.
        if !self.focused_once {
            self.focused_once = true;
            self.focus_handle.clone().focus(window, cx);
        }

        div()
            .id("terminal-workspace")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_add_panel))
            .on_action(cx.listener(Self::on_reset_layout))
            .on_action(cx.listener(Self::on_toggle_ai_chat))
            .on_action(cx.listener(Self::on_toggle_trading))
            .on_action(cx.listener(Self::on_ask_ai))
            .on_action(cx.listener(Self::on_apply_layout))
            .on_action(cx.listener(Self::on_save_layout))
            .on_action(cx.listener(Self::on_save_layout_current))
            .on_action(cx.listener(Self::on_manage_layouts))
            .on_action(cx.listener(Self::on_delete_layout))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .child(self.top_bar.clone())
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.dock_area.clone()),
            )
            .child(self.bottom_bar.clone())
            .children(sheet_layer)
            .children(dialog_layer)
            .children(notification_layer)
    }
}

fn try_load(
    dock_area: &Entity<DockArea>,
    window: &mut Window,
    cx: &mut Context<TerminalWorkspace>,
) -> bool {
    let Ok(Some(state)) = persistence::load() else {
        return false;
    };
    if state.version != Some(LAYOUT_VERSION) {
        return false;
    }
    dock_area
        .update(cx, |dock_area, cx| dock_area.load(state, window, cx))
        .is_ok()
}

fn apply_default_layout(
    dock_area: gpui::WeakEntity<DockArea>,
    window: &mut Window,
    cx: &mut App,
) {
    let layout = build_general_layout(&dock_area, window, cx);
    _ = dock_area.update(cx, |view, cx| {
        view.set_version(LAYOUT_VERSION, window, cx);
        view.set_center(layout, window, cx);
    });
}

fn build(kind: Kind, window: &mut Window, cx: &mut App) -> Arc<dyn PanelView> {
    panels::build_kind(kind, window, cx)
}

// ============================================================================
// Predefined layouts
// ============================================================================

/// Human-readable label for a preset id (the value we stash in
/// `CurrentLayoutKind::Predefined`). Falls back to the id itself for unknown
/// presets so newly added presets are at least diagnosable on the toolbar.
fn preset_display(id: &str) -> String {
    match id {
        PRESET_GENERAL => "General",
        PRESET_FUNDAMENTAL => "Fundamental",
        PRESET_TECHNICAL => "Technical",
        other => other,
    }
    .to_string()
}

/// Watchlist | Chart | Details — the original default; balanced for general use.
fn build_general_layout(
    dock_area: &gpui::WeakEntity<DockArea>,
    window: &mut Window,
    cx: &mut App,
) -> DockItem {
    let watchlist = build(Kind::Watchlist, window, cx);
    let chart = build(Kind::Chart, window, cx);
    let details = build(Kind::Details, window, cx);
    DockItem::split_with_sizes(
        Axis::Horizontal,
        vec![
            DockItem::tabs(vec![watchlist], dock_area, window, cx),
            DockItem::tabs(vec![chart], dock_area, window, cx),
            DockItem::tabs(vec![details], dock_area, window, cx),
        ],
        vec![Some(px(280.)), None, Some(px(320.))],
        dock_area,
        window,
        cx,
    )
}

/// Portfolio + Economic Calendar + News — fundamentals-heavy view.
fn build_fundamental_layout(
    dock_area: &gpui::WeakEntity<DockArea>,
    window: &mut Window,
    cx: &mut App,
) -> DockItem {
    let watchlist = build(Kind::Watchlist, window, cx);
    let portfolio = build(Kind::Portfolio, window, cx);
    let calendar = build(Kind::EconomicCalendar, window, cx);
    let news = build(Kind::NewsFeed, window, cx);
    DockItem::split_with_sizes(
        Axis::Horizontal,
        vec![
            DockItem::tabs(vec![watchlist], dock_area, window, cx),
            DockItem::tabs(vec![portfolio, calendar], dock_area, window, cx),
            DockItem::tabs(vec![news], dock_area, window, cx),
        ],
        vec![Some(px(260.)), None, Some(px(360.))],
        dock_area,
        window,
        cx,
    )
}

/// Watchlist | Chart (big) | SmartMoney + Details — chart/flow focused.
fn build_technical_layout(
    dock_area: &gpui::WeakEntity<DockArea>,
    window: &mut Window,
    cx: &mut App,
) -> DockItem {
    let watchlist = build(Kind::Watchlist, window, cx);
    let chart = build(Kind::Chart, window, cx);
    let smart = build(Kind::SmartMoney, window, cx);
    let details = build(Kind::Details, window, cx);
    DockItem::split_with_sizes(
        Axis::Horizontal,
        vec![
            DockItem::tabs(vec![watchlist], dock_area, window, cx),
            DockItem::tabs(vec![chart], dock_area, window, cx),
            DockItem::tabs(vec![smart, details], dock_area, window, cx),
        ],
        vec![Some(px(240.)), None, Some(px(320.))],
        dock_area,
        window,
        cx,
    )
}

fn adopt_from_item(item: &DockItem, cx: &App, ws: &mut TerminalWorkspace) {
    match item {
        DockItem::Split { items, .. } => {
            for child in items {
                adopt_from_item(child, cx, ws);
            }
        }
        DockItem::Tabs { items, .. } => {
            for panel in items {
                adopt_panel(panel, cx, ws);
            }
        }
        DockItem::Panel { view, .. } => adopt_panel(view, cx, ws),
        // Tiles layouts have a private `panel` field on TileItem, so we can't
        // reflect into them from outside the crate. Our default layout doesn't
        // use Tiles, so this is a non-issue today.
        DockItem::Tiles { .. } => {}
    }
}

fn notify_success(window: &mut Window, cx: &mut App, title: &str, body: String) {
    window.push_notification(
        gpui_component::notification::Notification::success(SharedString::from(body)).title(title),
        cx,
    );
}

fn notify_error(window: &mut Window, cx: &mut App, title: &str, body: &str) {
    window.push_notification(
        gpui_component::notification::Notification::error(SharedString::from(body)).title(title),
        cx,
    );
}

fn notify_warning(window: &mut Window, cx: &mut App, title: &str, body: &str) {
    window.push_notification(
        gpui_component::notification::Notification::warning(SharedString::from(body)).title(title),
        cx,
    );
}

fn notify_info(window: &mut Window, cx: &mut App, title: &str, body: &str) {
    window.push_notification(
        gpui_component::notification::Notification::info(SharedString::from(body)).title(title),
        cx,
    );
}

fn adopt_panel(panel: &Arc<dyn PanelView>, cx: &App, ws: &mut TerminalWorkspace) {
    let Ok(entity) = panel.view().downcast::<ContentPanel>() else {
        return;
    };
    let kind = entity.read(cx).kind();
    if !kind.is_singleton() {
        return;
    }
    let slot = ws.singleton_slot(kind);
    if slot.is_none() {
        *slot = Some(entity);
    }
    // If the slot already had something, the layout had duplicates — keep the
    // first one we encountered. (The duplicates will be orphaned and the user
    // can close them manually.)
}

/// Pin the parent TabPanel of `entity` so the user can't drag, drop, or close
/// the singleton via the tab strip. Toolbar toggles remain the only way to
/// remove or reposition it. Deferred because `parent_tab_panel` is set inside
/// `Panel::on_added_to`, which is itself dispatched via `window.defer` from
/// `StackPanel::insert_panel` — so the parent isn't wired up synchronously
/// when we return from `dock_to_window_edge`.
fn pin_panel_tab(
    entity: &Entity<ContentPanel>,
    window: &mut Window,
    cx: &mut Context<TerminalWorkspace>,
) {
    let weak = entity.downgrade();
    window.defer(cx, move |window, cx| {
        let Some(entity) = weak.upgrade() else { return };
        let Some(tp) = entity
            .read(cx)
            .parent_tab_panel()
            .and_then(|w| w.upgrade())
        else {
            return;
        };
        tp.update(cx, |tp, cx| tp.set_pinned(true, window, cx));
    });
}

/// Drop singleton ContentPanel entries from a serialized layout tree. Used
/// before persisting so AI Chat / Position / Execution don't leak into the
/// auto-saved layout or named user-saved layouts — those panels are managed
/// purely by the toolbar toggles.
fn prune_singletons_from_state(state: PanelState) -> Option<PanelState> {
    // Leaf panel: drop if it's a singleton kind.
    if state.children.is_empty()
        && matches!(state.info, PanelInfo::Panel(_))
        && Kind::from_id(&state.panel_name).is_some_and(|k| k.is_singleton())
    {
        return None;
    }

    let original_sizes: Option<Vec<gpui::Pixels>> = match &state.info {
        PanelInfo::Stack { sizes, .. } => Some(sizes.clone()),
        _ => None,
    };
    let original_active_ix = match &state.info {
        PanelInfo::Tabs { active_index } => Some(*active_index),
        _ => None,
    };

    let mut kept_children = Vec::new();
    let mut kept_sizes = Vec::new();
    for (i, child) in state.children.into_iter().enumerate() {
        if let Some(pruned) = prune_singletons_from_state(child) {
            kept_children.push(pruned);
            if let Some(sizes) = &original_sizes {
                if let Some(s) = sizes.get(i) {
                    kept_sizes.push(*s);
                }
            }
        }
    }

    // Containers that ended up empty disappear too, so the parent doesn't
    // render an empty StackPanel/TabPanel slot.
    if matches!(state.info, PanelInfo::Stack { .. } | PanelInfo::Tabs { .. })
        && kept_children.is_empty()
    {
        return None;
    }

    let info = match state.info {
        PanelInfo::Stack { axis, .. } => PanelInfo::Stack { sizes: kept_sizes, axis },
        PanelInfo::Tabs { .. } => {
            let max_ix = kept_children.len().saturating_sub(1);
            PanelInfo::Tabs {
                active_index: original_active_ix.unwrap_or(0).min(max_ix),
            }
        }
        other => other,
    };

    Some(PanelState {
        panel_name: state.panel_name,
        children: kept_children,
        info,
    })
}

/// Returns a copy of `state` with all singleton ContentPanels removed. If the
/// pruned center would be empty, the original is returned untouched (we never
/// want to write an empty layout). Skips the recursive clone+rebuild entirely
/// when no singleton is present in the tree.
fn dock_state_without_singletons(mut state: DockAreaState) -> DockAreaState {
    if !panel_tree_has_singleton(&state.center) {
        return state;
    }
    if let Some(pruned) = prune_singletons_from_state(state.center.clone()) {
        state.center = pruned;
    }
    state
}

fn panel_tree_has_singleton(state: &PanelState) -> bool {
    if matches!(state.info, PanelInfo::Panel(_))
        && Kind::from_id(&state.panel_name).is_some_and(|k| k.is_singleton())
    {
        return true;
    }
    state.children.iter().any(panel_tree_has_singleton)
}

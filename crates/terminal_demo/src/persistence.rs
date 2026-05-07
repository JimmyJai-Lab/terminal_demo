use std::collections::BTreeMap;

use anyhow::Result;
use gpui_component::dock::DockAreaState;
use serde::{Serialize, de::DeserializeOwned};

// ---------------------------------------------------------------------------
// Generic JSON helpers
// ---------------------------------------------------------------------------
//
// The layout / layouts / current_layout / font_size persistence entries all
// follow the same shape: a single JSON blob keyed by a stable string. These
// helpers wrap the cfg-gated localStorage ↔ filesystem split so individual
// callers (e.g. `load_layouts`, `save_current_layout`) stay one-liners.

#[cfg(target_family = "wasm")]
fn read_storage_blob(key: &str) -> Option<String> {
    web_sys::window()?.local_storage().ok()??.get_item(key).ok()?
}

#[cfg(target_family = "wasm")]
fn write_storage_blob(key: &str, value: &str) -> Result<()> {
    let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("no window"))?;
    let storage = window
        .local_storage()
        .map_err(|_| anyhow::anyhow!("localStorage unavailable"))?
        .ok_or_else(|| anyhow::anyhow!("localStorage unavailable"))?;
    storage
        .set_item(key, value)
        .map_err(|_| anyhow::anyhow!("localStorage write failed"))?;
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
fn config_path(file_name: &str) -> Result<std::path::PathBuf> {
    let dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("no config dir"))?
        .join("terminal_demo");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(file_name))
}

#[cfg(target_family = "wasm")]
fn load_json<T: DeserializeOwned + Default>(key: &str, _: &str) -> T {
    read_storage_blob(key)
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_default()
}

#[cfg(target_family = "wasm")]
fn save_json<T: Serialize>(key: &str, _: &str, value: &T) -> Result<()> {
    let json = serde_json::to_string(value)?;
    write_storage_blob(key, &json)
}

#[cfg(not(target_family = "wasm"))]
fn load_json<T: DeserializeOwned + Default>(_key: &str, file_name: &str) -> T {
    let Ok(path) = config_path(file_name) else {
        return T::default();
    };
    if !path.exists() {
        return T::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

#[cfg(not(target_family = "wasm"))]
fn save_json<T: Serialize>(_key: &str, file_name: &str, value: &T) -> Result<()> {
    let path = config_path(file_name)?;
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(&path, json)?;
    Ok(())
}

// localStorage keys (WASM) and JSON file names (native). The keys for layouts
// and current_layout are shared by the generic helpers below, so they aren't
// cfg-gated; the unused-on-native warning is silenced for STORAGE_KEY which
// is only consumed by the WASM-specific `load`/`save` impls.
#[cfg_attr(not(target_family = "wasm"), allow(dead_code))]
const STORAGE_KEY: &str = "terminal_demo.layout.v1";
#[cfg(target_family = "wasm")]
const FONT_SIZE_KEY: &str = "terminal_demo.font_size.v1";
const LAYOUTS_KEY: &str = "terminal_demo.layouts.v1";
const CURRENT_LAYOUT_KEY: &str = "terminal_demo.current_layout.v1";

/// Map of user-named layouts. BTreeMap so the menu shows them in stable
/// alphabetical order without an extra sort step.
pub type SavedLayouts = BTreeMap<String, DockAreaState>;

#[cfg(target_family = "wasm")]
pub fn load() -> Result<Option<DockAreaState>> {
    let Some(window) = web_sys::window() else {
        return Ok(None);
    };
    let storage = window
        .local_storage()
        .map_err(|_| anyhow::anyhow!("localStorage unavailable"))?
        .ok_or_else(|| anyhow::anyhow!("localStorage unavailable"))?;
    let Some(json) = storage
        .get_item(STORAGE_KEY)
        .map_err(|_| anyhow::anyhow!("localStorage read failed"))?
    else {
        return Ok(None);
    };
    let state: DockAreaState = serde_json::from_str(&json)?;
    Ok(Some(state))
}

#[cfg(target_family = "wasm")]
pub fn save(state: &DockAreaState) -> Result<()> {
    let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("no window"))?;
    let storage = window
        .local_storage()
        .map_err(|_| anyhow::anyhow!("localStorage unavailable"))?
        .ok_or_else(|| anyhow::anyhow!("localStorage unavailable"))?;
    let json = serde_json::to_string(state)?;
    storage
        .set_item(STORAGE_KEY, &json)
        .map_err(|_| anyhow::anyhow!("localStorage write failed"))?;
    Ok(())
}

#[cfg(target_family = "wasm")]
pub fn clear() -> Result<()> {
    let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("no window"))?;
    let storage = window
        .local_storage()
        .map_err(|_| anyhow::anyhow!("localStorage unavailable"))?
        .ok_or_else(|| anyhow::anyhow!("localStorage unavailable"))?;
    storage
        .remove_item(STORAGE_KEY)
        .map_err(|_| anyhow::anyhow!("localStorage remove failed"))?;
    Ok(())
}

#[cfg(target_family = "wasm")]
pub fn load_font_size() -> Option<f32> {
    let storage = web_sys::window()?.local_storage().ok()??;
    let raw = storage.get_item(FONT_SIZE_KEY).ok()??;
    raw.parse().ok()
}

#[cfg(target_family = "wasm")]
pub fn save_font_size(value: f32) -> Result<()> {
    let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("no window"))?;
    let storage = window
        .local_storage()
        .map_err(|_| anyhow::anyhow!("localStorage unavailable"))?
        .ok_or_else(|| anyhow::anyhow!("localStorage unavailable"))?;
    storage
        .set_item(FONT_SIZE_KEY, &value.to_string())
        .map_err(|_| anyhow::anyhow!("localStorage write failed"))?;
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
fn state_path() -> Result<std::path::PathBuf> {
    let dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("no config dir"))?
        .join("terminal_demo");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("layout.json"))
}

#[cfg(not(target_family = "wasm"))]
pub fn load() -> Result<Option<DockAreaState>> {
    let path = state_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let json = std::fs::read_to_string(&path)?;
    let state: DockAreaState = serde_json::from_str(&json)?;
    Ok(Some(state))
}

#[cfg(not(target_family = "wasm"))]
pub fn save(state: &DockAreaState) -> Result<()> {
    let path = state_path()?;
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, json)?;
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
pub fn clear() -> Result<()> {
    let path = state_path()?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
fn font_size_path() -> Result<std::path::PathBuf> {
    let dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("no config dir"))?
        .join("terminal_demo");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("font_size"))
}

#[cfg(not(target_family = "wasm"))]
pub fn load_font_size() -> Option<f32> {
    let path = font_size_path().ok()?;
    if !path.exists() {
        return None;
    }
    std::fs::read_to_string(&path).ok()?.trim().parse().ok()
}

#[cfg(not(target_family = "wasm"))]
pub fn save_font_size(value: f32) -> Result<()> {
    let path = font_size_path()?;
    std::fs::write(&path, value.to_string())?;
    Ok(())
}

// ============================================================================
// Named user layouts
// ============================================================================

pub fn load_layouts() -> SavedLayouts {
    load_json(LAYOUTS_KEY, "layouts.json")
}

pub fn save_layouts(layouts: &SavedLayouts) -> Result<()> {
    save_json(LAYOUTS_KEY, "layouts.json", layouts)
}

pub fn upsert_layout(name: &str, state: DockAreaState) -> Result<()> {
    let mut layouts = load_layouts();
    layouts.insert(name.to_string(), state);
    save_layouts(&layouts)
}

pub fn delete_layout(name: &str) -> Result<()> {
    let mut layouts = load_layouts();
    if layouts.remove(name).is_some() {
        save_layouts(&layouts)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Currently active layout name
// ---------------------------------------------------------------------------
//
// Stored as a tiny standalone JSON blob (`{"kind":"saved","name":"Foo"}`) so
// the toolbar can show the right label and the Save button can know whether
// to overwrite or pop the Save-As dialog.

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurrentLayoutKind {
    #[default]
    Unnamed,
    Predefined {
        id: String,
    },
    Saved {
        name: String,
    },
}

pub fn load_current_layout() -> CurrentLayoutKind {
    load_json(CURRENT_LAYOUT_KEY, "current_layout.json")
}

pub fn save_current_layout(value: &CurrentLayoutKind) -> Result<()> {
    save_json(CURRENT_LAYOUT_KEY, "current_layout.json", value)
}

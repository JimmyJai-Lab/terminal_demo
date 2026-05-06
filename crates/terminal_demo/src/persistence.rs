use anyhow::Result;
use gpui_component::dock::DockAreaState;

#[cfg(target_family = "wasm")]
const STORAGE_KEY: &str = "terminal_demo.layout.v1";

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

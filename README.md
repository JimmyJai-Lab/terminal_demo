# terminal_demo

Tiling-window financial-analytics workspace demo built with [gpui-component](https://github.com/longbridge/gpui-component). Runs natively and in the browser via WebAssembly.

## Run

Native (fast iteration):

```sh
make native
```

Web (Vite dev server at http://localhost:3000):

```sh
make install        # one-time: rustup wasm target + wasm-bindgen-cli + bun deps
make dev
```

## Layout

Three placeholder panels — **Watchlist** | **Chart** | **Details** — arranged horizontally. Drag a tab to a pane edge to split, drag tabs between panes to reorganize. Click `+ Panel` to spawn additional instances. `⋯` → **Reset Layout** restores the default.

State persists across reloads (localStorage in WASM, `~/.config/terminal_demo/layout.json` natively).

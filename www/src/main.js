async function init() {
  const loadingEl = document.getElementById('loading');
  const appEl = document.getElementById('app');

  try {
    const wasm = await import('./wasm/terminal_demo.js');
    await wasm.default();
    await wasm.run();

    if (appEl) appEl.remove();
  } catch (error) {
    console.error('Failed to initialize:', error);
    if (loadingEl) {
      loadingEl.innerHTML = `
        <div class="error">
          <h2>Failed to load terminal_demo</h2>
          <p>${error.message || error}</p>
          <p style="margin-top:10px; font-size:14px;">See console for details.</p>
        </div>
      `;
    }
  }
}

init();

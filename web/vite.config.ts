import { defineConfig, type Plugin } from 'vite';

// Documents never leave the browser. The production page enforces that with a Content Security Policy:
// scripts, styles, the wasm module and fetches come only from the app's own origin; previews may show
// images embedded in the document (data:) or generated here (blob:). The dev server needs inline
// scripts and a websocket for hot reload, so the policy is added to built pages only.
const policy = [
  "default-src 'self'",
  "script-src 'self' 'wasm-unsafe-eval'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data: blob:",
  "connect-src 'self'",
  "frame-src 'self' blob:",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'",
].join('; ');

function contentSecurityPolicy(): Plugin {
  return {
    name: 'off42-csp',
    apply: 'build',
    transformIndexHtml(html) {
      return html.replace(
        '<meta charset="UTF-8" />',
        `<meta charset="UTF-8" />\n    <meta http-equiv="Content-Security-Policy" content="${policy}" />`,
      );
    },
  };
}

export default defineConfig({
  base: './',
  plugins: [contentSecurityPolicy()],
  build: {
    target: 'es2022',
    // The core is one large wasm module; it is served as its own file next to the bundle.
    assetsInlineLimit: 0,
    chunkSizeWarningLimit: 20000,
  },
});

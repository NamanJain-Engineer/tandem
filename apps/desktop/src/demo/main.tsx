import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "../App";
import "../styles.css";
import { useUi } from "../stores/ui";
import { installDemo } from "./backend";
import { DemoBar } from "./DemoBar";
import { DemoHints } from "./DemoHints";

// vibezeo.com/demo: the real interface against a simulated core. On the home
// page it runs in a same-origin frame (`?embed=1`, D-128).
const core = installDemo();
const embedded = new URLSearchParams(window.location.search).has("embed");
// Small screens start with the rail collapsed, so every surface fits.
if (window.innerWidth < 1024) useUi.setState({ railCollapsed: true });
// Esc that nothing in the app used closes the home page's full-screen view.
if (embedded && window.parent !== window) {
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !e.defaultPrevented)
      window.parent.postMessage({ type: "vibezeo-demo:escape" }, window.location.origin);
  });
}
const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <div className="flex h-full flex-col">
        <DemoBar core={core} firstThread={core.firstThread} embedded={embedded} />
        <div className="relative min-h-0 flex-1">
          <App />
          <DemoHints />
        </div>
      </div>
    </StrictMode>,
  );
}

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "../App";
import "../styles.css";
import { installDemo } from "./backend";
import { DemoBar } from "./DemoBar";

// vibezeo.com/demo: the real interface against a simulated core.
const core = installDemo();
const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <div className="flex h-full flex-col">
        <DemoBar firstThread={core.firstThread} />
        <div className="relative min-h-0 flex-1">
          <App />
        </div>
      </div>
    </StrictMode>,
  );
}

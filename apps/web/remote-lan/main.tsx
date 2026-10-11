// The phone web app as the desktop serves it on the local network (F-RM-04,
// D-2004): the same RemoteApp as `/remote`, built by Vite into static files
// that the desktop bundles (`resources/remote-web`) and serves at
// `http://<lan-ip>:<port>/remote`.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RemoteApp } from "../app/remote/RemoteApp";
import "./styles.css";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <RemoteApp product="Vibezeo" />
    </StrictMode>,
  );
}

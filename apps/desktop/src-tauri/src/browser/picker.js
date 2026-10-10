// The element picker (F-CD-18), injected into a browser page while it's on.
// Hover highlights an element; click selects it, Shift+click adds it,
// Alt+click takes its parent instead, Esc stops. Each change goes back to
// the app as a navigation to tandem-picker://NONCE/<selection> from a hidden
// frame, which the browser takes and never loads: the page gets no IPC. A
// page whose Content-Security-Policy refuses the frame reports through a
// new-window request instead (clicks and Esc allow one), as does every page
// where the browser doesn't see a frame's navigations (VIA "window").
// Everything lives in this closure; its marks sit in a closed shadow root,
// out of the page's selectors.
(() => {
  const NONCE = "__TANDEM_NONCE__";
  const VIA = "__TANDEM_VIA__";
  const MAX_HTML = 4096;
  const doc = document;

  const host = doc.createElement("tandem-picker-layer");
  host.style.cssText = "position:fixed;inset:0;pointer-events:none;z-index:2147483647;";
  const root = host.attachShadow({ mode: "closed" });
  const mark = (color) => {
    const box = doc.createElement("div");
    box.style.cssText =
      "position:fixed;display:none;box-sizing:border-box;pointer-events:none;border-radius:2px;" +
      `border:2px solid ${color};background:${color}26;`;
    root.append(box);
    return box;
  };
  const hover = mark("#3b82f6");
  let picked = [];
  let marks = [];

  const place = (box, el) => {
    const r = el.getBoundingClientRect();
    Object.assign(box.style, { display: "block", left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, height: `${r.height}px` });
  };
  const redraw = () => {
    for (const m of marks) m.remove();
    marks = picked.map((el) => {
      const box = mark("#f59e0b");
      place(box, el);
      return box;
    });
  };

  const escape = (s) => (window.CSS && CSS.escape ? CSS.escape(s) : String(s).replace(/[^\w-]/g, (c) => `\\${c}`));
  const only = (sel, el) => {
    try {
      const all = doc.querySelectorAll(sel);
      return all.length === 1 && all[0] === el;
    } catch {
      return false;
    }
  };
  const step = (el) => {
    const tag = el.localName;
    const parent = el.parentElement;
    if (!parent) return tag;
    const same = Array.from(parent.children).filter((c) => c.localName === tag);
    return same.length === 1 ? tag : `${tag}:nth-of-type(${same.indexOf(el) + 1})`;
  };

  /** A selector that matches `el` and nothing else: a unique id, else a
   * path of tags (with their place among siblings of the same tag) from the
   * nearest uniquely named ancestor, shortened while it stays unique. */
  const selectorOf = (el) => {
    if (el.id && only(`#${escape(el.id)}`, el)) return `#${escape(el.id)}`;
    const parts = [];
    let node = el;
    for (; node && node !== doc.documentElement; node = node.parentElement) {
      if (node !== el && node.id && only(`#${escape(node.id)}`, node)) {
        parts.unshift(`#${escape(node.id)}`);
        break;
      }
      parts.unshift(step(node));
    }
    if (!node || node === doc.documentElement) parts.unshift("html");
    let best = parts.join(" > ");
    for (let i = 1; i < parts.length; i++) {
      const shorter = parts.slice(i).join(" > ");
      if (!only(shorter, el)) break;
      best = shorter;
    }
    return best;
  };

  /** The React or Vue component that rendered `el`, if the page says. */
  const componentOf = (el) => {
    for (let node = el; node && node.nodeType === 1; node = node.parentElement) {
      const key = Object.keys(node).find((k) => k.startsWith("__reactFiber$") || k.startsWith("__reactInternalInstance$"));
      if (key) {
        for (let fiber = node[key]; fiber; fiber = fiber.return) {
          const type = fiber.type;
          const fn = typeof type === "function" ? type : type && typeof type === "object" ? type.render || type.type : null;
          const name = (type && type.displayName) || (fn && (fn.displayName || fn.name));
          if (typeof fn === "function" && name) return { name, framework: "React" };
        }
        return null;
      }
      const vue3 = node.__vueParentComponent;
      const vue3Name = vue3 && vue3.type && (vue3.type.name || vue3.type.__name);
      if (vue3Name) return { name: vue3Name, framework: "Vue" };
      const vue2 = node.__vue__;
      const vue2Name = vue2 && vue2.$options && (vue2.$options.name || vue2.$options._componentTag);
      if (vue2Name) return { name: vue2Name, framework: "Vue" };
    }
    return null;
  };

  /** CSS's shortest form of four sides. */
  const sides = (t, r, b, l) => (t === r && r === b && b === l ? t : t === b && r === l ? `${t} ${r}` : r === l ? `${t} ${r} ${b}` : `${t} ${r} ${b} ${l}`);

  const describe = (el) => {
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    const font = [cs.fontStyle === "normal" ? "" : cs.fontStyle, cs.fontWeight, cs.lineHeight === "normal" ? cs.fontSize : `${cs.fontSize}/${cs.lineHeight}`, cs.fontFamily]
      .filter(Boolean)
      .join(" ");
    const image = cs.backgroundImage && cs.backgroundImage !== "none" ? ` ${cs.backgroundImage}` : "";
    const html = el.outerHTML;
    return {
      tag: el.localName,
      selector: selectorOf(el),
      component: componentOf(el),
      rect: { x: Math.round(r.left + window.scrollX), y: Math.round(r.top + window.scrollY), width: Math.round(r.width), height: Math.round(r.height) },
      styles: {
        display: cs.display,
        position: cs.position,
        font,
        color: cs.color,
        background: `${cs.backgroundColor}${image}`,
        margin: sides(cs.marginTop, cs.marginRight, cs.marginBottom, cs.marginLeft),
        padding: sides(cs.paddingTop, cs.paddingRight, cs.paddingBottom, cs.paddingLeft),
      },
      html: html.length > MAX_HTML ? html.slice(0, MAX_HTML) : html,
      truncated: html.length > MAX_HTML,
    };
  };

  let viaWindow = VIA === "window";
  const onViolation = (e) => {
    if (String(e.blockedURI).startsWith("tandem-picker")) viaWindow = true;
  };
  /** Each message holds the whole selection, so a lost one costs nothing. */
  const send = (stopped) => {
    const url = `tandem-picker://${NONCE}/${encodeURIComponent(JSON.stringify({ elements: picked.map(describe), stopped }))}`;
    if (viaWindow) {
      window.open(url, "_blank");
      return;
    }
    const frame = doc.createElement("iframe");
    frame.style.display = "none";
    frame.src = url;
    root.append(frame);
    setTimeout(() => frame.remove(), 1000);
  };

  const target = (e) => {
    const el = e.target;
    return el instanceof Element && el !== host ? el : null;
  };
  const swallow = (e) => {
    if (!target(e)) return;
    e.preventDefault();
    e.stopPropagation();
    e.stopImmediatePropagation();
  };
  const onMove = (e) => {
    const el = target(e);
    if (el) place(hover, el);
  };
  const onClick = (e) => {
    let el = target(e);
    if (!el) return;
    swallow(e);
    if (e.altKey && el.parentElement && el.parentElement !== doc.documentElement) el = el.parentElement;
    if (e.shiftKey) {
      if (!picked.includes(el)) picked = [...picked, el];
    } else picked = [el];
    redraw();
    send(false);
  };
  const onKey = (e) => {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    send(true);
    stop();
  };
  const onScroll = () => {
    redraw();
    hover.style.display = "none";
  };

  const swallowed = ["pointerdown", "pointerup", "mousedown", "mouseup", "dblclick", "auxclick"];
  const stop = () => {
    doc.removeEventListener("mousemove", onMove, true);
    doc.removeEventListener("click", onClick, true);
    doc.removeEventListener("keydown", onKey, true);
    for (const type of swallowed) doc.removeEventListener(type, swallow, true);
    window.removeEventListener("scroll", onScroll, true);
    window.removeEventListener("resize", onScroll);
    doc.removeEventListener("securitypolicyviolation", onViolation, true);
    host.remove();
  };

  doc.addEventListener("mousemove", onMove, true);
  doc.addEventListener("click", onClick, true);
  doc.addEventListener("keydown", onKey, true);
  for (const type of swallowed) doc.addEventListener(type, swallow, true);
  window.addEventListener("scroll", onScroll, true);
  window.addEventListener("resize", onScroll);
  doc.addEventListener("securitypolicyviolation", onViolation, true);
  doc.addEventListener(`tandem-picker-stop:${NONCE}`, stop, { once: true });
  doc.documentElement.append(host);
  // The empty selection at once: a page whose CSP refuses the frame says so
  // now, before the first pick needs it.
  if (!viaWindow) send(false);
})();

import { useEffect, useMemo, useState } from "react";
import { MODE_COPY, SHELL_COMMANDS, type ShellMode } from "./commands";

const RAIL: { mode: ShellMode; label: string }[] = [
  { mode: "home", label: "Home" },
  { mode: "agents", label: "Agents" },
  { mode: "code", label: "Code" },
  { mode: "threads", label: "Threads" },
];

export function AppShell() {
  const [mode, setMode] = useState<ShellMode>("home");
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [query, setQuery] = useState("");

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen(true);
      } else if (event.key === "Escape") {
        setPaletteOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const matches = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return SHELL_COMMANDS.filter((command) => command.title.toLowerCase().includes(needle));
  }, [query]);

  const copy = MODE_COPY[mode];

  return (
    <div className="shell">
      <nav className="rail" aria-label="Modes">
        {RAIL.map((item) => (
          <button
            key={item.mode}
            type="button"
            aria-pressed={mode === item.mode}
            onClick={() => setMode(item.mode)}
          >
            {item.label}
          </button>
        ))}
      </nav>
      <aside className="sidebar">
        <h2>{copy.title}</h2>
        <p>Nothing in this list yet.</p>
      </aside>
      <main className="canvas">
        <h1>{copy.title}</h1>
        <p>{copy.body}</p>
      </main>
      <footer className="status">
        <span>Tandem</span>
        <span>{copy.title}</span>
        <span>local</span>
        <button type="button" onClick={() => setPaletteOpen(true)}>
          Commands
        </button>
      </footer>
      {paletteOpen ? (
        <div className="palette-backdrop" onClick={() => setPaletteOpen(false)}>
          <div
            className="palette"
            role="dialog"
            aria-label="Commands"
            onClick={(event) => event.stopPropagation()}
          >
            <input
              autoFocus
              aria-label="Filter commands"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
            <ul>
              {matches.length === 0 ? <li>No matches</li> : null}
              {matches.map((command) => (
                <li key={command.id}>
                  <button
                    type="button"
                    role="option"
                    onClick={() => {
                      setMode(command.mode);
                      setQuery("");
                      setPaletteOpen(false);
                    }}
                  >
                    {command.title}
                  </button>
                </li>
              ))}
            </ul>
          </div>
        </div>
      ) : null}
    </div>
  );
}

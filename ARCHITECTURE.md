# Architecture

Tandem is one repository. Packages are the split boundary. A second repository is the wrong move until a service has its own release cadence and on-call rotation. Until then, a monorepo keeps the local store, the sync API, and the shell on one commit.

## Boundaries

| Module | Owns | Must not own |
|---|---|---|
| `tandem-core` | Paths, redaction, containment, logs | Product tables, HTTP |
| `tandem-engine` | How a coding CLI is found on `PATH` | Process supervision, tokens |
| `tandem-store` | Local SQLite: workspaces, sessions, panes, agents, the event log | Network |
| `api` | Stateless HTTP. Sync log when `DATABASE_URL` is set, in-memory otherwise | Local files, CLI tokens |
| `desktop` | The shell frame the window host will wrap | Business rules that belong in the store |
| `web` | Account and health surface | The desktop shell |
| `@tandem/protocol` | Wire types shared by `api` and clients | UI |

## How this scales

- API processes hold no product state when `DATABASE_URL` points at Postgres. Add nodes behind a load balancer. Rate limits belong at that edge, not in process memory.
- Sync is an append-only log. Clients send their own event ids, so retries do not duplicate. Readers pass `after=<seq>`. The sequence is assigned by the database, so concurrent writers do not collide.
- The same cursor shape exists locally (`events.id`) and in the cloud (`sync_events.seq`). A later sync worker uploads local events and pulls remote ones. It does not need a new data model.
- Engines are a registry, not a switch buried in the UI. A new CLI is a new `Engine` entry plus a supervisor, which is not built yet.
- SQLite stays on the machine that has the files. Postgres is only the multi-device log.

## Not in this tree yet

The window host (Tauri) is not compiled here. This environment has no GTK or WebKit, and a crate that cannot build would fail CI. `apps/desktop` is the UI that host will load on port 1420.

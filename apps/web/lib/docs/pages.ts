/** Every /docs page, in the sidebar's order (F-WB-02). `slug` "" is /docs/. */
export const DOCS_PAGES = [
  { slug: "", title: "Getting started" },
  { slug: "code-mode", title: "Code mode" },
  { slug: "workflow-tools", title: "Checkpoints and workflow tools" },
  { slug: "thread-mode", title: "Thread mode" },
  { slug: "agent-mode", title: "Agent mode" },
  { slug: "dictate", title: "Dictate" },
  { slug: "conductor", title: "Conductor" },
  { slug: "tasks", title: "Tasks" },
  { slug: "routines", title: "Routines" },
  { slug: "plugins", title: "Plugins and approvals" },
  { slug: "accounts-and-limits", title: "Accounts and limits" },
  { slug: "remote", title: "Remote (SSH, WSL, phone)" },
  { slug: "privacy", title: "Privacy and data flows" },
  { slug: "keyboard-shortcuts", title: "Keyboard shortcuts" },
  { slug: "settings", title: "Settings" },
  { slug: "engines", title: "Engines and platforms" },
  { slug: "troubleshooting", title: "Troubleshooting" },
] as const;

export const docsPath = (slug: string) => `/docs/${slug ? `${slug}/` : ""}`;

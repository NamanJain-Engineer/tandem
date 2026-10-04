export type ShellMode = "home" | "agents" | "code" | "threads";

export type ShellCommand = {
  id: string;
  title: string;
  mode: ShellMode;
};

export const SHELL_COMMANDS: ShellCommand[] = [
  { id: "nav.home", title: "Open Home", mode: "home" },
  { id: "nav.agents", title: "Open Agents", mode: "agents" },
  { id: "nav.code", title: "Open Code", mode: "code" },
  { id: "nav.threads", title: "Open Threads", mode: "threads" },
];

export const MODE_COPY: Record<ShellMode, { title: string; body: string }> = {
  home: {
    title: "Home",
    body: "Open a project folder. Tandem keeps that work on this machine.",
  },
  agents: {
    title: "Agents",
    body: "Named teammates, with their own instructions and memory, will live here.",
  },
  code: {
    title: "Code",
    body: "Terminal panes for the coding CLIs you already run.",
  },
  threads: {
    title: "Threads",
    body: "Readable sessions, without the raw terminal scrollback, will live here.",
  },
};

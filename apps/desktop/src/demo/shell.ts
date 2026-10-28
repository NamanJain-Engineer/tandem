/** A tiny line-editing shell for the browser demo's terminals: it echoes
 * keys, handles Backspace, Ctrl+C and Ctrl+L, and answers a few commands
 * from the sample repository as its checkout has it. Nothing runs. */

import type { StoryCommit } from "./story";

const ESC = "\x1b";
const GREEN = `${ESC}[32m`;
const RED = `${ESC}[31m`;
const YELLOW = `${ESC}[33m`;
const BLUE = `${ESC}[34m`;
const CYAN = `${ESC}[36m`;
const DIM = `${ESC}[2m`;
const BOLD = `${ESC}[1m`;
const RESET = `${ESC}[0m`;

/** What the shell can see of its checkout. */
export type ShellPlace = {
  /** As the prompt shows it: "~/shop/.tandem/worktrees/coupon-rules". */
  dir: string;
  /** `pwd`. */
  path: string;
  branch: string;
  /** Every file in the folder now. */
  paths: () => string[];
  text: (path: string) => string | undefined;
  /** `git status`'s files. */
  changes: () => Array<{ path: string; change: string }>;
  /** `git diff` of one changed file. */
  diff: (path: string) => string;
  /** The branch's commits, newest first. */
  log: () => StoryCommit[];
  /** `pnpm test [filter]`'s output. */
  test: (filter: string) => string[];
};

const color = (line: string) =>
  line.startsWith("+") && !line.startsWith("+++")
    ? `${GREEN}${line}${RESET}`
    : line.startsWith("-") && !line.startsWith("---")
      ? `${RED}${line}${RESET}`
      : line.startsWith("@@")
        ? `${CYAN}${line}${RESET}`
        : line;

/** One terminal's line editor. `write` gets what xterm sends and returns
 * what the "PTY" echoes back. */
export class DemoShell {
  private line = "";

  constructor(private readonly place: ShellPlace) {}

  prompt(): string {
    return `${GREEN}you@vibezeo${RESET}:${BLUE}${this.place.dir}${RESET}$ `;
  }

  greeting(): string {
    return `${DIM}Simulated shell, type ${RESET}${BOLD}help${RESET}${DIM} to see what it knows.${RESET}\r\n${this.prompt()}`;
  }

  /** Canned answers; `null` clears the screen. */
  run(input: string): string[] | null {
    const [cmd = "", ...args] = input.trim().split(/\s+/);
    const p = this.place;
    switch (cmd) {
      case "":
        return [];
      case "help":
        return [
          `${BOLD}This terminal is part of a browser demo.${RESET} It understands:`,
          "  ls, cat, pwd, git status, git diff, git log, pnpm test, echo, whoami, date, clear",
          `${DIM}In Vibezeo every pane is a real terminal on your machine.${RESET}`,
        ];
      case "ls": {
        const dir = (args[0] ?? "").replace(/\/+$/, "");
        const prefix = dir ? `${dir}/` : "";
        const names = new Set<string>();
        for (const path of p.paths()) {
          if (!path.startsWith(prefix)) continue;
          const rest = path.slice(prefix.length);
          const at = rest.indexOf("/");
          names.add(at >= 0 ? `${rest.slice(0, at)}/` : rest);
        }
        if (names.size === 0) return [`ls: cannot access '${dir}': No such file or directory`];
        return [[...names].sort().map((n) => (n.endsWith("/") ? `${BLUE}${n}${RESET}` : n)).join("  ")];
      }
      case "cat": {
        if (!args[0]) return ["cat: which file? Try cat package.json"];
        const text = p.text(args[0]);
        return text === undefined ? [`cat: ${args[0]}: No such file or directory`] : text.replace(/\n$/, "").split("\n");
      }
      case "pwd":
        return [p.path];
      case "whoami":
        return ["you"];
      case "date":
        return [new Date().toString()];
      case "echo":
        return [args.join(" ")];
      case "clear":
        return null;
      case "git": {
        if (args[0] === "status") {
          const changes = p.changes();
          const modified = changes.filter((c) => c.change !== "untracked");
          const untracked = changes.filter((c) => c.change === "untracked");
          return [
            `On branch ${p.branch}`,
            ...(modified.length
              ? ["Changes not staged for commit:", ...modified.map((c) => `  ${RED}modified:   ${c.path}${RESET}`)]
              : []),
            ...(untracked.length ? ["Untracked files:", ...untracked.map((c) => `  ${RED}${c.path}${RESET}`)] : []),
            ...(changes.length ? [] : ["nothing to commit, working tree clean"]),
          ];
        }
        if (args[0] === "diff") {
          // Like git: changes to tracked files; new files wait for `git add`.
          const lines = p
            .changes()
            .filter((c) => c.change !== "untracked")
            .flatMap((c) => p.diff(c.path).replace(/\n$/, "").split("\n"));
          return lines.map(color);
        }
        if (args[0] === "log") return p.log().map((c) => `${YELLOW}${c.sha}${RESET} ${c.subject}`);
        return [`git: '${args[0] ?? ""}' isn't available in this demo. Try git status, git diff or git log.`];
      }
      case "npm":
      case "pnpm":
        if (args[0] === "test" || args[0] === "t") return p.test(args.slice(1).join(" "));
        return [`${cmd} ${args.join(" ")}: only \`${cmd} test\` works in this demo.`];
      case "claude":
      case "codex":
      case "gemini":
        return [
          `${cmd} would start here in Vibezeo. This demo has no real agents:`,
          "each worktree's agent pane shows what they look like.",
        ];
      default:
        return [`${cmd}: command not found (type ${BOLD}help${RESET})`];
    }
  }

  write(data: string): string {
    let out = "";
    for (const ch of data) {
      if (ch === "\r") {
        const result = this.run(this.line);
        this.line = "";
        if (result === null) out += `${ESC}[2J${ESC}[H${this.prompt()}`;
        else out += `\r\n${result.map((l) => `${l}\r\n`).join("")}${this.prompt()}`;
      } else if (ch === "\x7f" || ch === "\b") {
        if (this.line.length > 0) {
          this.line = this.line.slice(0, -1);
          out += "\b \b";
        }
      } else if (ch === "\x03") {
        this.line = "";
        out += `^C\r\n${this.prompt()}`;
      } else if (ch === "\x0c") {
        out += `${ESC}[2J${ESC}[H${this.prompt()}${this.line}`;
      } else if (ch === ESC) {
        // Arrow keys and other sequences: ignored.
        break;
      } else if (ch >= " ") {
        this.line += ch;
        out += ch;
      }
    }
    return out;
  }
}

const SGR = new RegExp(`${ESC}\\[[0-9;]*m`, "g");

/** `text` as an agent's terminal UI shows it `cols` wide: each line broken
 * at spaces and continued at its indent, where the terminal would cut a
 * word in two. Colour codes take no room; a word wider than the pane stays
 * whole. */
export function wrapWords(text: string, cols: number): string {
  const width = (s: string) => s.replace(SGR, "").length;
  return text
    .split("\n")
    .map((line) => {
      if (width(line) <= cols) return line;
      const indent = /^ */.exec(line)?.[0] ?? "";
      const out: string[] = [];
      let cur = "";
      for (const word of line.slice(indent.length).split(" ")) {
        if (cur && width(`${indent}${cur} ${word}`) > cols) {
          out.push(indent + cur);
          cur = word;
        } else cur = cur ? `${cur} ${word}` : word;
      }
      out.push(indent + cur);
      return out.join("\n");
    })
    .join("\n");
}

/** What an agent pane shows: a still frame of an agent's terminal UI. */
export function agentScreen(engine: string, task: string, lines: string[]): string {
  // As wide as its words, so it fits a narrow pane without wrapping.
  const label = ` ${engine} · simulated in this demo `;
  const bar = "─".repeat(label.length);
  return [
    `${DIM}╭${bar}╮${RESET}`,
    `${DIM}│${RESET} ${BOLD}${engine}${RESET} ${DIM}· simulated in this demo${RESET} ${DIM}│${RESET}`,
    `${DIM}╰${bar}╯${RESET}`,
    "",
    `${BOLD}>${RESET} ${task}`,
    "",
    ...lines,
    "",
  ].join("\r\n");
}

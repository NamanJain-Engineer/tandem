import { useEffect, useState, type ReactNode } from "react";
import { Bell, Folder, PanelLeft, Plus, Settings, Terminal } from "lucide-react";
import { ApprovalCard } from "../components/ApprovalCard";
import { Avatar, AvatarStack } from "../components/Avatar";
import { Badge, Chip } from "../components/Badge";
import { WaveformRing } from "../components/WaveformRing";
import { Button } from "../components/Button";
import { Checkbox, RadioGroup, Switch } from "../components/Choice";
import { CommandPalette } from "../components/CommandPalette";
import { QuickPicker } from "../components/QuickPicker";
import { Banner, Callout, EmptyState, KeyValue, ListRow, PageHeader, SettingsGroup, SettingsRow, Stepper } from "../components/Composites";
import { ConfirmDialog, Dialog, Sheet } from "../components/Dialog";
import { Composer } from "../components/Composer";
import { DiffView } from "../components/DiffView";
import { CodeEditor } from "../components/CodeEditor";
import { FileTree, type FileTreeNode } from "../components/FileTree";
import { Input, SecretInput, Textarea } from "../components/Field";
import { IconButton } from "../components/IconButton";
import { Kbd } from "../components/Kbd";
import { CodeBlock, MarkdownView } from "../components/MarkdownView";
import { Menu } from "../components/Menu";
import { ContextMeter, ProgressBar, UsageMeter } from "../components/Progress";
import { SegmentedControl } from "../components/SegmentedControl";
import { Combobox, Select } from "../components/Select";
import { Skeleton } from "../components/Skeleton";
import { Spinner } from "../components/Spinner";
import { StatusDot, type Status } from "../components/StatusDot";
import { Tabs } from "../components/Tabs";
import { ToolCallBlock, ToolGroup, ToolOutput } from "../components/ToolCallBlock";
import { useToast, type ToastTone } from "../components/Toast";
import { Tooltip } from "../components/Tooltip";

/** Every component state, rendered by `/__gallery` and checked by the
 * snapshot + axe test (UI_SPEC §0, §7.6). Copy here is sample content. */
export type GalleryState = { name: string; render: () => ReactNode };
export type GalleryEntry = { id: string; title: string; spec: string; states: GalleryState[] };

const noop = () => undefined;

function ShowToast({ tone }: { tone: ToastTone }) {
  const { show } = useToast();
  useEffect(() => {
    show({ tone, title: `Sample ${tone} toast`, body: "Body text for the toast.", action: { label: "Undo", onClick: noop } });
  }, [show, tone]);
  return null;
}

function Controlled<T>({ initial, children }: { initial: T; children: (v: T, set: (v: T) => void) => ReactNode }) {
  const [v, set] = useState(initial);
  return <>{children(v, set)}</>;
}

const STATUSES: Status[] = ["working", "waiting", "done", "idle", "error", "offline"];
const MODES = [
  { id: "agent", label: "Agent", kbd: ["Mod", "1"] },
  { id: "code", label: "Code", kbd: ["Mod", "2"] },
  { id: "thread", label: "Thread", kbd: ["Mod", "3"] },
];

const TREE: Record<string, FileTreeNode[] | undefined> = {
  "": [
    { path: "node_modules", name: "node_modules", kind: "dir", ignored: true },
    { path: "src", name: "src", kind: "dir", gitStatus: "modified" },
    { path: "docs", name: "docs", kind: "dir", gitStatus: "untracked" },
    { path: ".env", name: ".env", kind: "file" },
    { path: "README.md", name: "README.md", kind: "file", gitStatus: "modified" },
    { path: "config.json", name: "config.json", kind: "file", symlink: true },
  ],
  src: [
    { path: "src/auth", name: "auth", kind: "dir" },
    { path: "src/main.rs", name: "main.rs", kind: "file", gitStatus: "modified" },
    { path: "src/new.rs", name: "new.rs", kind: "file", gitStatus: "added" },
  ],
  "src/auth": undefined,
};

export const galleryEntries: GalleryEntry[] = [
  {
    id: "button",
    title: "Button",
    spec: "§2.1",
    states: [
      ...(["primary", "secondary", "ghost", "danger", "link"] as const).map((v) => ({
        name: v,
        render: () => <Button variant={v}>Continue</Button>,
      })),
      { name: "sizes", render: () => (<div className="flex gap-2"><Button size="sm">Small</Button><Button size="md">Medium</Button><Button size="lg">Large</Button></div>) },
      { name: "loading", render: () => <Button variant="primary" loading>Saving</Button> },
      { name: "disabled", render: () => <Button disabled disabledReason="Included with Pro">Connect</Button> },
      { name: "with kbd and icon", render: () => <Button variant="primary" iconLeft={<Plus size={14} />} kbd="⌘↩">Send</Button> },
    ],
  },
  {
    id: "icon-button",
    title: "IconButton",
    spec: "§2.2",
    states: [
      { name: "ghost", render: () => <IconButton icon={<Settings size={16} />} label="Settings" /> },
      { name: "pressed", render: () => <IconButton icon={<PanelLeft size={16} />} label="Toggle left sidebar" pressed kbd={["Mod", "\\"]} /> },
      { name: "badge", render: () => <IconButton icon={<Bell size={16} />} label="Notifications, 3 waiting" badge={3} badgeTone="waiting" /> },
      { name: "badge 99+", render: () => <IconButton icon={<Bell size={16} />} label="Notifications, 120 unread" badge={120} /> },
      { name: "disabled", render: () => <IconButton icon={<Plus size={16} />} label="Add" disabled /> },
    ],
  },
  {
    id: "input",
    title: "Input / Textarea / SecretInput",
    spec: "§2.3",
    states: [
      { name: "default", render: () => <Input label="Workspace name" placeholder="shop" /> },
      { name: "hint", render: () => <Input label="Extra launch arguments" hint="Separated by spaces" monospace defaultValue="--verbose" /> },
      { name: "error", render: () => <Input label="Email" error="Enter a valid email address" defaultValue="ana@" /> },
      { name: "prefix + clearable", render: () => <Controlled initial="scroll">{(v, set) => <Input label="Filter settings" hideLabel prefix={<Folder size={14} />} clearable value={v} onChange={(e) => set(e.target.value)} onClear={() => set("")} />}</Controlled> },
      { name: "disabled", render: () => <Input label="Gateway port" disabled defaultValue="127.0.0.1:43120" /> },
      { name: "secret", render: () => <SecretInput label="API key" defaultValue="hidden-value" /> },
      { name: "textarea", render: () => <Textarea label="About you" minRows={3} defaultValue="Line one" /> },
    ],
  },
  {
    id: "select",
    title: "Select / Combobox",
    spec: "§2.4",
    states: [
      {
        name: "select",
        render: () => (
          <Select
            label="Terminal theme"
            value="follow_app"
            options={[
              { value: "follow_app", label: "Follow app" },
              { value: "night", label: "Night" },
              { value: "day", label: "Day" },
              { value: "high_contrast", label: "High contrast", disabled: true, disabledReason: "Turn on high contrast in your OS" },
            ]}
          />
        ),
      },
      { name: "select placeholder", render: () => <Select ariaLabel="Engine" value="" placeholder="Choose an engine" options={[{ value: "shell", label: "Shell" }]} /> },
      { name: "combobox", render: () => <Combobox ariaLabel="Terminal font" value="jb" options={[{ value: "jb", label: "JetBrains Mono" }, { value: "menlo", label: "Menlo" }]} /> },
    ],
  },
  {
    id: "choice",
    title: "Checkbox / Radio / Switch",
    spec: "§2.5",
    states: [
      { name: "checkbox", render: () => <Checkbox label="Send a crash report" checked={false} /> },
      { name: "checkbox checked", render: () => <Checkbox label="Also remove engine sign-ins" checked description="Signs out every engine account" /> },
      { name: "checkbox indeterminate", render: () => <Checkbox label="All sessions" checked="indeterminate" /> },
      { name: "radio", render: () => <RadioGroup label="Density" value="compact" items={[{ value: "compact", label: "Compact" }, { value: "comfortable", label: "Comfortable" }]} /> },
      { name: "switch off", render: () => <Switch label="Launch at login" checked={false} /> },
      { name: "switch on", render: () => <Switch label="Open links in Tandem" checked /> },
      { name: "switch disabled", render: () => <Switch label="Conductor" checked={false} disabled /> },
    ],
  },
  {
    id: "tabs",
    title: "Tabs",
    spec: "§2.6",
    states: [
      { name: "underline", render: () => <Tabs label="Sidebar" value="dash" items={[{ id: "dash", label: "Dashboard", count: 2 }, { id: "files", label: "Files" }, { id: "tasks", label: "Tasks" }]} renderPanel={(id) => <p className="p-2">Panel {id}</p>} /> },
      { name: "pill", render: () => <Tabs label="Usage" variant="pill" value="week" items={[{ id: "day", label: "Day" }, { id: "week", label: "Week" }]} renderPanel={(id) => <p className="p-2">{id}</p>} /> },
    ],
  },
  {
    id: "segmented",
    title: "SegmentedControl",
    spec: "§2.7",
    states: [
      { name: "mode switch", render: () => <SegmentedControl label="Work mode" modeSwitch value="code" items={MODES} /> },
      { name: "small", render: () => <SegmentedControl label="Text size" size="sm" value="default" items={[{ id: "small", label: "Small" }, { id: "default", label: "Default" }, { id: "large", label: "Large" }]} /> },
    ],
  },
  {
    id: "tooltip",
    title: "Tooltip",
    spec: "§2.8",
    states: [{ name: "open", render: () => <Tooltip label="Command palette" kbd={["Mod", "K"]} open><button type="button">Trigger</button></Tooltip> }],
  },
  {
    id: "menu",
    title: "Menu",
    spec: "§2.9",
    states: [
      {
        name: "open",
        render: () => (
          <Menu
            open
            trigger={<Button>Workspace</Button>}
            entries={[
              { id: "remove", label: "Remove from Tandem", danger: true },
              { id: "label", type: "label", label: "Layout" },
              { id: "arrange", label: "Auto-arrange panes", kbd: ["Mod", "Shift", "T"] },
              { id: "cards", label: "Card view" },
              { id: "sessions", type: "checkbox", label: "Show terminal sessions", checked: true },
              { id: "usage", label: "Usage…", disabled: true },
            ]}
          />
        ),
      },
    ],
  },
  {
    id: "dialog",
    title: "Dialog / ConfirmDialog",
    spec: "§2.10",
    states: [
      {
        name: "dialog",
        render: () => (
          <Dialog open onOpenChange={noop} title="Choose a plan" description="Your coding agents always run on your own AI accounts." footer={<><Button variant="ghost">Later</Button><Button variant="primary">Continue</Button></>}>
            <p>Dialog body.</p>
          </Dialog>
        ),
      },
      { name: "confirm", render: () => <ConfirmDialog open onOpenChange={noop} title="Remove shop from Tandem?" body="Nothing on disk is deleted." confirmLabel="Remove" onConfirm={noop} /> },
      { name: "confirm danger typed", render: () => <ConfirmDialog open onOpenChange={noop} danger requireText="delete" title="Delete all Tandem data on this device?" body="Your project folders aren't touched." confirmLabel="Delete data" onConfirm={noop} /> },
    ],
  },
  {
    id: "sheet",
    title: "Sheet",
    spec: "§2.11",
    states: [{ name: "open", render: () => <Sheet open onOpenChange={noop} title="Launch agents" subtitle="shop" footer={<Button variant="primary">Launch</Button>}><p>Sheet body.</p></Sheet> }],
  },
  {
    id: "toast",
    title: "Toast",
    spec: "§2.12",
    states: (["info", "success", "warning", "error"] as const).map((tone) => ({ name: tone, render: () => <ShowToast tone={tone} /> })),
  },
  {
    id: "badge",
    title: "Badge / Chip",
    spec: "§2.13",
    states: [
      { name: "badges", render: () => (<div className="flex flex-wrap gap-2">{(["neutral", "accent", "working", "waiting", "done", "idle", "error", "outline"] as const).map((v) => <Badge key={v} variant={v} dot={v !== "outline"}>{v}</Badge>)}</div>) },
      { name: "chips", render: () => (<div className="flex gap-2"><Chip label="screenshot.png" onRemove={noop} /><Chip label="src/auth.ts" icon={<Folder size={12} />} selected /></div>) },
    ],
  },
  {
    id: "avatar",
    title: "Avatar",
    spec: "§2.14",
    states: [
      { name: "sizes", render: () => (<div className="flex items-end gap-2">{([20, 24, 32, 48, 72] as const).map((s) => <Avatar key={s} name="Release Lead" id={`tm_${s}`} size={s} />)}</div>) },
      { name: "status", render: () => <Avatar name="Ana" id="tm_ana" size={32} status="waiting" /> },
      { name: "stack", render: () => <AvatarStack people={[{ name: "Ana", id: "a" }, { name: "Bo", id: "b" }, { name: "Cy", id: "c" }, { name: "Di", id: "d" }]} /> },
    ],
  },
  {
    id: "status-dot",
    title: "StatusDot",
    spec: "§2.15",
    states: [{ name: "all", render: () => (<div className="flex items-center gap-3">{STATUSES.map((s) => <StatusDot key={s} status={s} size={10} />)}</div>) }],
  },
  {
    id: "progress",
    title: "ProgressBar / UsageMeter",
    spec: "§2.16",
    states: [
      { name: "progress", render: () => <ProgressBar label="Downloading 0.2.0" value={0.42} /> },
      { name: "indeterminate", render: () => <ProgressBar label="Checking for updates" /> },
      { name: "meters", render: () => (<div className="flex flex-col gap-2"><UsageMeter label="5h" value={0.62} resetText="resets in 1h 12m" /><UsageMeter label="Week" value={0.86} /><UsageMeter label="Context" value={0.97} kind="context" /><UsageMeter label="5h" size="xs" value={0.97} /></div>) },
      { name: "context", render: () => (<div className="flex gap-4"><ContextMeter label="Context" used={114_000} window={200_000} /><ContextMeter label="Context" used={170_000} window={200_000} /></div>) },
    ],
  },
  {
    id: "empty",
    title: "EmptyState",
    spec: "§2.17",
    states: [{ name: "default", render: () => <EmptyState icon={<Folder size={20} />} title="Add a project folder" body="Tandem works on folders on your disk. Agents' changes land right there." primary={<Button variant="primary">Add folder…</Button>} secondary={<Button>Clone repository…</Button>} /> }],
  },
  {
    id: "loading",
    title: "Skeleton / Spinner",
    spec: "§2.18",
    states: [{ name: "default", render: () => (<div className="flex flex-col gap-2"><Skeleton width={200} /><Skeleton width={140} /><Spinner label="Loading" size={20} /></div>) }],
  },
  {
    id: "kbd",
    title: "Kbd",
    spec: "§2.19",
    states: [{ name: "default", render: () => (<div className="flex gap-3"><Kbd keys={["Mod", "K"]} /><Kbd keys={["Mod", "Shift", "B"]} /><Kbd keys="Esc" /></div>) }],
  },
  {
    id: "palette",
    title: "CommandPalette",
    spec: "§2.22",
    states: [
      {
        name: "open",
        render: () => (
          <CommandPalette
            open
            onOpenChange={noop}
            groups={[
              { heading: "Go to", items: [{ id: "go.code", label: "Code mode", kbd: ["Mod", "2"], onSelect: noop }] },
              { heading: "Actions", prefix: ">", items: [{ id: "ws.add", label: "Add folder…", icon: <Plus size={16} />, onSelect: noop }, { id: "term.new", label: "New terminal", icon: <Terminal size={16} />, onSelect: noop }] },
              { heading: "Settings", items: [{ id: "settings.open", label: "Open Settings", kbd: ["Mod", ","], onSelect: noop }] },
            ]}
          />
        ),
      },
    ],
  },
  {
    id: "quick-picker",
    title: "QuickPicker",
    spec: "§2.22 (UI-141, UI-145)",
    states: [
      {
        name: "open",
        render: () => (
          <QuickPicker
            open
            onOpenChange={noop}
            title="Recall a command"
            placeholder="Search commands you've run"
            query=""
            onQueryChange={noop}
            onSelect={noop}
            items={[
              { id: "1", label: "pnpm test", content: <span className="font-mono text-sm">pnpm test</span> },
              { id: "2", label: "docker compose up -d", content: <span className="font-mono text-sm">docker compose up -d</span> },
            ]}
            hints={[{ keys: ["Enter"], label: "Insert" }, { keys: ["Mod", "Enter"], label: "Run" }]}
          />
        ),
      },
    ],
  },
  {
    id: "composites",
    title: "Composites",
    spec: "§2.31",
    states: [
      { name: "list rows", render: () => (<div role="list" className="w-64"><div role="listitem"><ListRow leading={<Folder size={14} />} title="shop" secondary="main" trailing={<Badge>2</Badge>} /></div><div role="listitem"><ListRow leading={<StatusDot status="working" />} title="Fix auth guard" selected depth={1} /></div></div>) },
      { name: "page header", render: () => <PageHeader title="Appearance" subtitle="How Tandem looks." actions={<Button size="sm">Reset</Button>} /> },
      {
        name: "settings rows",
        render: () => (
          <SettingsGroup label="Terminal">
            <SettingsRow label="Scrollback lines" description="1,000–100,000" highlighted control={({ id, describedBy }) => <input id={id} aria-describedby={describedBy} type="number" defaultValue={10000} className="h-8 w-24 rounded-sm border border-border bg-surface-4 px-2 text-fg" />} />
            <SettingsRow label="Conductor" badge={<Badge variant="accent">Pro</Badge>} disabledReason="Included with Pro" control={({ id, describedBy }) => <Switch id={id} aria-describedby={describedBy} checked={false} disabled />} />
          </SettingsGroup>
        ),
      },
      ...(["info", "warning", "error", "success"] as const).map((tone) => ({
        name: `banner ${tone}`,
        render: () => <Banner tone={tone} action={{ label: "Retry", onClick: noop }} onDismiss={noop}>You're offline. Local features keep working.</Banner>,
      })),
      { name: "callout", render: () => <Callout tone="accent" title="Free includes 3 workspaces" actions={<Button size="sm" variant="primary">See plans</Button>}>Remove one from the rail (nothing on disk is deleted) or upgrade.</Callout> },
      { name: "key value", render: () => <KeyValue items={[{ key: "Plan", value: "PRO · renews 12 Oct" }, { key: "Credits", value: "8,420 of 10,000" }]} /> },
      { name: "stepper", render: () => <Stepper steps={["Theme", "Engines", "Dictate", "Workspace"]} current={1} /> },
    ],
  },
  {
    id: "waveform-ring",
    title: "WaveformRing",
    spec: "§2.30",
    states: [
      { name: "idle", render: () => <WaveformRing state="idle" label="Dictate" /> },
      { name: "listening", render: () => <WaveformRing state="listening" level={0.6} size={56} label="Listening" /> },
      { name: "error", render: () => <WaveformRing state="error" label="Your microphone isn't delivering audio" /> },
      { name: "onboarding", render: () => <WaveformRing state="idle" size={120} label="Dictate" /> },
    ],
  },
  {
    id: "diff-view",
    title: "DiffView",
    spec: "§2.25",
    states: [
      {
        name: "unified",
        render: () => (
          <DiffView
            className="w-[560px]"
            files={[
              {
                path: "src/auth/guard.ts",
                change: "modified",
                additions: 2,
                deletions: 1,
                binary: false,
                truncated: false,
                patch: "@@ -10,4 +10,5 @@ export function guard(req) {\n   const token = read(req);\n-  if (!token) return deny();\n+  if (!token || expired(token)) return deny();\n+  refresh(token);\n   return allow(token);\n }",
              },
              { path: "assets/logo.png", change: "added", additions: 0, deletions: 0, binary: true, truncated: false, patch: "" },
            ]}
          />
        ),
      },
      { name: "empty", render: () => <DiffView files={[]} /> },
    ],
  },
  {
    id: "file-tree",
    title: "FileTree",
    spec: "§2.26",
    states: [
      {
        name: "populated",
        render: () => (
          <div className="flex h-[320px] w-[280px]">
            <FileTree entries={TREE} expanded={new Set(["src", "src/auth"])} selected="src/main.rs" onToggle={noop} onOpen={noop} />
          </div>
        ),
      },
      {
        name: "loading",
        render: () => (
          <div className="flex h-[120px] w-[280px]">
            <FileTree entries={{}} expanded={new Set()} onToggle={noop} onOpen={noop} />
          </div>
        ),
      },
      {
        name: "empty",
        render: () => (
          <div className="flex h-[120px] w-[280px]">
            <FileTree entries={{ "": [] }} expanded={new Set()} onToggle={noop} onOpen={noop} />
          </div>
        ),
      },
      {
        name: "error",
        render: () => (
          <div className="flex h-[120px] w-[280px]">
            <FileTree entries={{}} errors={{ "": "Permission denied" }} expanded={new Set()} onToggle={noop} onOpen={noop} onRetry={noop} />
          </div>
        ),
      },
    ],
  },
  {
    id: "code-editor",
    title: "CodeEditor",
    spec: "§2.26 (Files editor)",
    states: [
      {
        name: "editable",
        render: () => (
          <div className="h-[160px] w-[480px]">
            <CodeEditor docKey="main" path="src/main.rs" label="Editor: src/main.rs" value={"fn main() {\n    println!(\"hi\");\n}\n"} />
          </div>
        ),
      },
      {
        name: "read-only",
        render: () => (
          <div className="h-[120px] w-[480px]">
            <CodeEditor docKey="hosts" path="/etc/hosts" label="Editor: /etc/hosts (read-only)" readOnly value={"127.0.0.1 localhost\n"} />
          </div>
        ),
      },
    ],
  },
  {
    id: "markdown-view",
    title: "MarkdownView",
    spec: "§2.31",
    states: [
      {
        name: "agent message",
        render: () => (
          <MarkdownView
            className="w-[560px]"
            markdown={"## The redirect loop\n\nThe guard calls `refresh()` **before** checking expiry, so an expired token redirects forever.\n\n- [x] Reproduce with `pnpm test checkout`\n- [ ] Fix the guard\n\n```ts\nif (!token || expired(token)) return deny();\n```\n\n| File | Change |\n|:--|--:|\n| src/auth/guard.ts | +2 −1 |\n\nSee [the docs](https://vibezeo.com/docs)."}
          />
        ),
      },
      { name: "streaming", render: () => <MarkdownView className="w-[560px]" streaming markdown={"First paragraph is done.\n\nThe second one is still being wri"} /> },
      { name: "code block", render: () => <CodeBlock className="w-[480px]" language="sh" code={"pnpm install\npnpm test"} /> },
    ],
  },
  {
    id: "tool-call",
    title: "ToolCallBlock",
    spec: "UI-21",
    states: [
      { name: "running", render: () => <ToolCallBlock className="w-[480px]" icon="command" verb="Running" arg="pnpm test checkout" status="running" statusLabel="Running" /> },
      { name: "done", render: () => <ToolCallBlock className="w-[480px]" icon="read" verb="Read" arg="src/checkout.ts" status="ok" statusLabel="Done" /> },
      {
        name: "failed expanded",
        render: () => (
          <ToolCallBlock className="w-[480px]" icon="command" verb="Ran" arg="pnpm test checkout" meta="exit 1, 6s" status="error" statusLabel="Failed" defaultOpen>
            <ToolOutput text={"FAIL  test/checkout.test.ts\n  ✕ redirects once (12 ms)"} note="Showing the last 64 KB" />
          </ToolCallBlock>
        ),
      },
      { name: "denied", render: () => <ToolCallBlock className="w-[480px]" icon="edit" verb="Blocked" arg="src/checkout.ts" status="denied" statusLabel="Blocked" /> },
      {
        name: "group",
        render: () => (
          <div className="w-[480px]">
            <ToolGroup summary="4 tool calls" collapseLabel="Collapse" status="ok" statusLabel="Done" defaultOpen>
              <ToolCallBlock icon="read" verb="Read" arg="src/a.ts" status="ok" statusLabel="Done" />
              <ToolCallBlock icon="search" verb="Searched for" arg="redirect" meta="(3 files)" status="ok" statusLabel="Done" />
              <ToolCallBlock icon="web" verb="Fetched" arg="example.com" status="ok" statusLabel="Done" />
            </ToolGroup>
          </div>
        ),
      },
    ],
  },
  {
    id: "approval-card",
    title: "ApprovalCard",
    spec: "§2.24",
    states: [
      {
        name: "command",
        render: () => (
          <ApprovalCard
            className="w-[560px]"
            kind="command"
            title="Claude Code wants to run a command"
            detail={<CodeBlock code="pnpm prisma migrate reset" language="sh" />}
            context="In: ~/code/shop · Branch: tandem/fix-auth"
            risk="high"
            riskText="This can delete data."
            age="2m"
            allowThread
            onAllow={noop}
            onAllowThread={noop}
            onDeny={noop}
          />
        ),
      },
      {
        name: "file edit",
        render: () => (
          <ApprovalCard
            className="w-[560px]"
            kind="file_write"
            title="Codex wants to edit 2 files"
            detail={<CodeBlock code="--- a/src/a.ts" />}
            labels={{ allow: "Allow", showDetail: "View diff", hideDetail: "Hide diff" }}
            onAllow={noop}
            onDeny={noop}
          />
        ),
      },
      { name: "compact", render: () => <ApprovalCard className="w-[560px]" compact kind="command" title="Claude Code wants to run npm test" allowThread onAllow={noop} onAllowThread={noop} onDeny={noop} /> },
      { name: "resolved", render: () => <ApprovalCard kind="command" title="Run npm test" onAllow={noop} onDeny={noop} resolved={{ tone: "allowed", text: "Allowed once · you · 10:42" }} /> },
    ],
  },
  {
    id: "composer",
    title: "Composer",
    spec: "§2.23",
    states: [
      {
        name: "idle",
        render: () => (
          <Controlled initial="">
            {(v, set) => (
              <Composer
                className="w-[720px]"
                value={v}
                onChange={set}
                onSend={noop}
                placeholder="Ask Claude Code about shop — @ a file or folder, paste a screenshot…"
                mode="act"
                engine={{ value: "claude", options: [{ value: "claude", label: "Claude Code" }, { value: "codex", label: "Codex" }], onChange: noop }}
                model={{ value: "default", options: [{ value: "default", label: "Default" }], onChange: noop }}
                effort={{ value: "medium", options: [{ value: "medium", label: "Medium" }], onChange: noop }}
                access={{ value: "edits", options: [{ value: "read", label: "Read only" }, { value: "edits", label: "Edits" }, { value: "full", label: "Full access" }], onChange: noop }}
                tokens={{ text: "12.4k", tooltip: "Tokens used in this thread · 10.1k in / 2.3k out" }}
                onAttach={noop}
                mentions={() => []}
              />
            )}
          </Controlled>
        ),
      },
      {
        name: "running with queue",
        render: () => (
          <Composer
            className="w-[720px]"
            value="and then update the docs"
            onChange={noop}
            onSend={noop}
            onStop={noop}
            running
            queued="Run the tests again"
            onRemoveQueued={noop}
            attachments={[
              { id: "a", label: "error.png", kind: "image" },
              { id: "b", label: "src/checkout.ts", kind: "file" },
              { id: "c", label: "video.mov", kind: "file", error: "Too large (max 20 MB)" },
            ]}
            onRemoveAttachment={noop}
            placeholder="Working… you can type your next message, or Stop."
          />
        ),
      },
      { name: "disabled", render: () => <Composer className="w-[720px]" value="" onChange={noop} onSend={noop} placeholder="Ask anything…" disabledReason="Claude Code isn't signed in." /> },
      { name: "drop", render: () => <Composer className="w-[720px]" value="" onChange={noop} onSend={noop} placeholder="Ask anything…" dropActive /> },
    ],
  },
];

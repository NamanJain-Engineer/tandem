import {
  Activity,
  Bell,
  BookText,
  FolderTree,
  GitBranch,
  History,
  KeyRound,
  Layers,
  ListTodo,
  Lock,
  Mic,
  Network,
  Package,
  PanelTop,
  Plug,
  RotateCcw,
  Search,
  Server,
  ShieldCheck,
  Sparkles,
  SquareTerminal,
  Users,
  type LucideIcon,
} from "lucide-react";

/** The icon names lib/home.ts uses, kept out of the data so it stays plain. */
export const ICONS: Record<string, LucideIcon> = {
  activity: Activity,
  bell: Bell,
  book: BookText,
  branch: GitBranch,
  files: FolderTree,
  history: History,
  key: KeyRound,
  layers: Layers,
  lock: Lock,
  mic: Mic,
  network: Network,
  package: Package,
  plug: Plug,
  restore: RotateCcw,
  search: Search,
  server: Server,
  shield: ShieldCheck,
  sparkles: Sparkles,
  tasks: ListTodo,
  terminal: SquareTerminal,
  tray: PanelTop,
  users: Users,
};

export function iconFor(name: string): LucideIcon {
  const icon = ICONS[name];
  if (!icon) throw new Error(`no icon "${name}"`);
  return icon;
}

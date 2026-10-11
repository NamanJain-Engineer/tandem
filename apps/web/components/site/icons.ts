import {
  Activity,
  Bell,
  FolderTree,
  GitBranch,
  KeyRound,
  ListTodo,
  Lock,
  Mic,
  PanelTop,
  Plug,
  RotateCcw,
  Search,
  Server,
  ShieldCheck,
  Users,
  type LucideIcon,
} from "lucide-react";

/** The icon names lib/home.ts uses, kept out of the data so it stays plain. */
export const ICONS: Record<string, LucideIcon> = {
  activity: Activity,
  bell: Bell,
  branch: GitBranch,
  files: FolderTree,
  key: KeyRound,
  lock: Lock,
  mic: Mic,
  plug: Plug,
  restore: RotateCcw,
  search: Search,
  server: Server,
  shield: ShieldCheck,
  tasks: ListTodo,
  tray: PanelTop,
  users: Users,
};

export function iconFor(name: string): LucideIcon {
  const icon = ICONS[name];
  if (!icon) throw new Error(`no icon "${name}"`);
  return icon;
}

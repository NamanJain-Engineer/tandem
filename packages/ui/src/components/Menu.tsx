import { ContextMenu as CM, DropdownMenu as DM } from "radix-ui";
import { Check, ChevronRight } from "lucide-react";
import type { ReactElement, ReactNode } from "react";
import { cx } from "../cx";
import { Kbd } from "./Kbd";

export type MenuEntry =
  | {
      type?: "item";
      id: string;
      label: string;
      icon?: ReactNode;
      kbd?: string[];
      danger?: boolean;
      disabled?: boolean;
      onSelect?: () => void;
      submenu?: MenuEntry[];
    }
  | { type: "checkbox"; id: string; label: string; checked: boolean; disabled?: boolean; onCheckedChange?: (v: boolean) => void }
  | { type: "separator"; id: string }
  | { type: "label"; id: string; label: string };

const PANEL = "tn-pop z-[var(--z-popover)] min-w-[200px] rounded-md border border-border bg-surface-3 p-1 shadow-2";
const ITEM =
  "relative flex h-7 cursor-default select-none items-center gap-2 rounded-sm px-2 text-base text-fg outline-none data-[disabled]:opacity-40 data-[highlighted]:bg-surface-4";

/** Destructive items always go last, after a separator (UI_SPEC §2.9). */
export function orderEntries(entries: MenuEntry[]): MenuEntry[] {
  const isDanger = (e: MenuEntry) => (e.type === undefined || e.type === "item") && e.danger;
  const safe = entries.filter((e) => !isDanger(e));
  const danger = entries.filter(isDanger);
  while (safe.length && safe[safe.length - 1]?.type === "separator") safe.pop();
  return danger.length ? [...safe, { type: "separator", id: "__danger" }, ...danger] : safe;
}

type Parts = typeof DM | typeof CM;

function renderEntries(P: Parts, entries: MenuEntry[]): ReactNode {
  const Item = P.Item as typeof DM.Item;
  const Sub = P.Sub as typeof DM.Sub;
  const SubTrigger = P.SubTrigger as typeof DM.SubTrigger;
  const SubContent = P.SubContent as typeof DM.SubContent;
  const Separator = P.Separator as typeof DM.Separator;
  const Label = P.Label as typeof DM.Label;
  const CheckboxItem = P.CheckboxItem as typeof DM.CheckboxItem;
  const ItemIndicator = P.ItemIndicator as typeof DM.ItemIndicator;
  const Portal = P.Portal as typeof DM.Portal;
  return orderEntries(entries).map((e) => {
    if (e.type === "separator") return <Separator key={e.id} className="my-1 h-px bg-divider" />;
    if (e.type === "label")
      return (
        <Label key={e.id} className="px-2 pb-1 pt-2 text-2xs font-semibold uppercase tracking-caps text-fg-subtle">
          {e.label}
        </Label>
      );
    if (e.type === "checkbox")
      return (
        <CheckboxItem key={e.id} checked={e.checked} disabled={e.disabled} onCheckedChange={(v) => e.onCheckedChange?.(v === true)} className={cx(ITEM, "pl-7")}>
          <span className="absolute left-2 inline-flex">
            <ItemIndicator>
              <Check size={14} aria-hidden="true" />
            </ItemIndicator>
          </span>
          {e.label}
        </CheckboxItem>
      );
    if (e.submenu)
      return (
        <Sub key={e.id}>
          <SubTrigger disabled={e.disabled} className={cx(ITEM, "data-[state=open]:bg-surface-4")}>
            {e.icon && <span aria-hidden="true">{e.icon}</span>}
            <span className="flex-1">{e.label}</span>
            <ChevronRight size={14} aria-hidden="true" />
          </SubTrigger>
          <Portal>
            <SubContent className={PANEL} sideOffset={4}>
              {renderEntries(P, e.submenu)}
            </SubContent>
          </Portal>
        </Sub>
      );
    return (
      <Item key={e.id} disabled={e.disabled} onSelect={() => e.onSelect?.()} className={cx(ITEM, e.danger && "text-danger")}>
        {e.icon && <span aria-hidden="true" className="inline-flex">{e.icon}</span>}
        <span className="flex-1 truncate">{e.label}</span>
        {e.kbd && <Kbd keys={e.kbd} />}
      </Item>
    );
  });
}

export type MenuProps = {
  trigger: ReactElement;
  entries: MenuEntry[];
  align?: "start" | "center" | "end";
  side?: "top" | "right" | "bottom" | "left";
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  /** Closing focuses the trigger again unless this prevents it (an item
   * that moves focus somewhere new). */
  onCloseAutoFocus?: (event: Event) => void;
};

/** Dropdown menu (UI_SPEC §2.9). */
export function Menu({ trigger, entries, align = "start", side = "bottom", open, onOpenChange, onCloseAutoFocus }: MenuProps) {
  return (
    <DM.Root open={open} onOpenChange={onOpenChange}>
      <DM.Trigger asChild>{trigger}</DM.Trigger>
      <DM.Portal>
        <DM.Content align={align} side={side} sideOffset={4} className={PANEL} onCloseAutoFocus={onCloseAutoFocus}>
          {renderEntries(DM, entries)}
        </DM.Content>
      </DM.Portal>
    </DM.Root>
  );
}

/** The same menu on right-click, Shift+F10 or the Menu key. */
export function ContextMenu({ children, entries }: { children: ReactElement; entries: MenuEntry[] }) {
  return (
    <CM.Root>
      <CM.Trigger asChild>{children}</CM.Trigger>
      <CM.Portal>
        <CM.Content className={PANEL}>{renderEntries(CM, entries)}</CM.Content>
      </CM.Portal>
    </CM.Root>
  );
}

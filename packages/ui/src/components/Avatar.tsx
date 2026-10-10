import { cx } from "../cx";
import { StatusDot, type Status } from "./StatusDot";

/** FNV-1a, 32-bit. */
export function fnv1a32(text: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
}

/**
 * The 12-hue palette (F-AG-01, D-302): evenly spread, none in the reds near
 * the error colour (UI_SPEC §1.7 skips 0–15 and 345–360).
 */
export const AVATAR_HUES = [25, 52, 80, 108, 136, 164, 192, 220, 248, 276, 304, 332] as const;

/** The palette index for an id: the user's pick, else `fnv1a32(id) % 12`. */
export function hueIndex(id: string, picked?: number | null): number {
  if (picked !== undefined && picked !== null && picked >= 0 && picked < AVATAR_HUES.length) return picked;
  return fnv1a32(id) % AVATAR_HUES.length;
}

/** The hue an avatar draws with. */
export function avatarHue(id: string, picked?: number | null): number {
  return AVATAR_HUES[hueIndex(id, picked)] ?? AVATAR_HUES[0];
}

export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  if (words.length === 1) return (words[0] ?? "").slice(0, 2).toUpperCase();
  return `${words[0]?.[0] ?? ""}${words[1]?.[0] ?? ""}`.toUpperCase();
}

export type AvatarProps = {
  name: string;
  id: string;
  size?: 20 | 24 | 32 | 48 | 72;
  status?: Status;
  imageUrl?: string;
  /** A picked palette index (0–11); the id decides otherwise. */
  hue?: number | null;
  className?: string;
};

export function Avatar({ name, id, size = 24, status, imageUrl, hue, className }: AvatarProps) {
  const h = avatarHue(id, hue);
  return (
    <span className={cx("relative inline-flex shrink-0", className)} style={{ width: size, height: size }}>
      {imageUrl ? (
        <img src={imageUrl} alt={name} className="size-full rounded-md object-cover" />
      ) : (
        <span
          role="img"
          aria-label={name}
          className="inline-flex size-full items-center justify-center rounded-md font-semibold text-white"
          style={{
            background: `linear-gradient(135deg, hsl(${h} 65% 48%), hsl(${(h + 40) % 360} 70% 38%))`,
            fontSize: Math.round(size * 0.42),
          }}
        >
          <span aria-hidden="true">{initials(name)}</span>
        </span>
      )}
      {status && (
        <span className="absolute -bottom-0.5 -right-0.5 inline-flex rounded-full bg-surface-2 p-[2px]">
          <StatusDot status={status} size={size >= 48 ? 10 : size >= 32 ? 8 : 6} />
        </span>
      )}
    </span>
  );
}

export function AvatarStack({ people, size = 24 }: { people: Array<{ name: string; id: string }>; size?: AvatarProps["size"] }) {
  const shown = people.slice(0, 3);
  const more = people.length - shown.length;
  return (
    <span className="inline-flex items-center">
      {shown.map((p, i) => (
        <Avatar key={p.id} name={p.name} id={p.id} size={size} className={cx(i > 0 && "-ml-1.5 rounded-md ring-2 ring-surface-2")} />
      ))}
      {more > 0 && <span className="ml-1 text-sm text-fg-muted">+{more}</span>}
    </span>
  );
}

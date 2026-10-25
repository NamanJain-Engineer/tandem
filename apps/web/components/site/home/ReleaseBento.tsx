import { versionPath, PLATFORM_LABEL } from "../../../lib/changelog";
import { releaseTiles, tilesRelease } from "../../../lib/home";
import { cx } from "../../../lib/cx";
import { DOWNLOADS, RELEASE } from "../../../lib/release";
import { CommandLine } from "../CommandLine";
import { Icon } from "../Icon";
import { iconFor } from "../icons";
import { InlineMarkdown } from "../InlineMarkdown";
import { Section } from "../Section";
import { TextLink } from "../TextLink";

const SPAN = ["lg:col-span-4", "lg:col-span-2"] as const;
const REST: Record<number, string> = { 2: "lg:col-span-2", 4: "lg:col-span-4", 6: "lg:col-span-6" };

/** #new (§6.8): the release's Added entries as tiles, generated from
 * CHANGELOG.md (title = the bold lead phrase, body = its first sentence),
 * plus the real install command. A release that only changes and fixes
 * things shows the last feature release's tiles, under its version. */
export function ReleaseBento() {
  const release = tilesRelease();
  const tiles = releaseTiles(release);
  if (!release || tiles.length === 0) return null;
  const current = release.version === RELEASE.version;
  const install = DOWNLOADS.find((d) => d.os === "linux" && d.available)?.steps.find((s) => s.includes("apt install"));
  const command = install?.replace(/^[^:]*:\s*/, "");
  const used = tiles.reduce((n, _, i) => n + (i === 0 ? 4 : 2), 0) % 6;
  const rest = used === 0 ? 6 : 6 - used;
  return (
    <Section
      id="new"
      tone="raised"
      eyebrow={`New in ${release.version}`}
      title={current ? "What's new in this release" : `What ${release.version} added`}
      aside={<TextLink href={versionPath(release.version)}>Release notes</TextLink>}
    >
      <ul className="grid gap-4 sm:grid-cols-2 lg:grid-cols-6">
        {tiles.map((t, i) => (
          <li
            key={t.title}
            className={cx(
              "reveal surface-raised relative isolate flex min-w-0 flex-col gap-4 overflow-hidden p-6",
              i === 0 ? "sm:col-span-2 lg:p-9" : "",
              SPAN[i === 0 ? 0 : 1],
            )}
          >
            {i === 0 && <div aria-hidden="true" className="bg-lens absolute inset-0 -z-10" />}
            <div className="flex items-center justify-between gap-3">
              <span className="inline-flex size-10 items-center justify-center rounded-[10px] text-fg-muted shadow-[inset_0_1px_0_var(--site-edge-hi),inset_0_0_0_1px_var(--site-line-strong)] [background:linear-gradient(180deg,var(--tn-accent-soft),transparent)]">
                <Icon icon={iconFor(t.icon)} size={20} />
              </span>
              <span className="font-mono text-[0.6875rem] uppercase tracking-[0.12em] text-fg-subtle">
                {t.platforms.map((p) => PLATFORM_LABEL[p as keyof typeof PLATFORM_LABEL] ?? p).join(" · ")}
              </span>
            </div>
            <h3 className={cx("text-fg", i === 0 ? "text-display-md" : "text-title")}>{t.title}</h3>
            <p className={cx("flex-1 text-pretty text-fg-muted [&_code]:font-mono [&_code]:text-[0.875em]", i === 0 ? "text-copy" : "text-copy-sm")}>
              <InlineMarkdown text={t.body} />
            </p>
            <TextLink href={t.docs} className="self-start text-copy-sm">
              Read the docs<span className="sr-only">: {t.title}</span>
            </TextLink>
          </li>
        ))}
        {command && (
          <li className={cx("reveal surface-raised flex min-w-0 flex-col justify-center gap-4 p-6 sm:col-span-2", REST[rest])}>
            <p className="text-title text-fg">Install {RELEASE.version} on Debian or Ubuntu</p>
            <CommandLine command={command} />
          </li>
        )}
      </ul>
    </Section>
  );
}

import { useEffect, useMemo, useState } from "react";
import { CATEGORIES, parseChangelog, type Platform } from "@tandem/protocol/changelog";
import { Button, Dialog, MarkdownView } from "@tandem/ui";
import changelogText from "../../../../../CHANGELOG.md?raw";
import { t } from "../../i18n";
import { useSettings } from "../../stores/settings";
import { useBoot } from "../shell/useBoot";

type AppNotes = { summary: string; sections: Array<{ category: string; items: string[] }> };

const PLATFORM: Record<string, Exclude<Platform, "all">> = { linux: "lin", macos: "mac", windows: "win" };

/** One version's notes for this OS, from CHANGELOG.md (F-WB-03); `null`
 * when the version has no section (a dev build) or nothing for this OS. */
export function notesForApp(text: string, version: string, os: string): AppNotes | null {
  const release = parseChangelog(text).find((r) => r.version === version);
  const platform = PLATFORM[os];
  if (!release || !platform) return null;
  const sections = CATEGORIES.map((category) => ({
    category,
    items: release.entries
      .filter(
        (e) => e.category === category && (e.platforms.includes("all") || e.platforms.includes(platform)),
      )
      .map((e) => e.text),
  })).filter((s) => s.items.length > 0);
  return sections.length ? { summary: release.summary, sections } : null;
}

/** "What's new" (F-WB-03): once per version, after an update. A first
 * install shows nothing and just remembers the version. */
export function WhatsNew({ changelog = changelogText }: { changelog?: string }) {
  const version = useBoot((s) => s.boot?.version ?? null);
  const os = useBoot((s) => s.boot?.platform ?? "");
  const loaded = useSettings((s) => s.loaded);
  const seen = useSettings((s) => s.values.whats_new_seen as string | null | undefined);
  const setSetting = useSettings((s) => s.set);
  const [open, setOpen] = useState(false);
  const notes = useMemo(
    () => (version ? notesForApp(changelog, version, os) : null),
    [changelog, version, os],
  );

  useEffect(() => {
    if (!loaded || !version || seen === version) return;
    if (seen && notes) setOpen(true);
    else void setSetting("whats_new_seen", version);
  }, [loaded, version, seen, notes, setSetting]);

  if (!notes || !version) return null;
  const markdown = [
    notes.summary,
    ...notes.sections.map((s) => [`### ${s.category}`, ...s.items.map((i) => `- ${i}`)].join("\n")),
  ]
    .filter(Boolean)
    .join("\n\n");
  const close = () => {
    setOpen(false);
    void setSetting("whats_new_seen", version);
  };
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => !o && close()}
      size="lg"
      title={t("whatsNew.title", { version })}
      closeLabel={t("common.close")}
      footer={
        <Button variant="primary" onClick={close}>
          {t("whatsNew.done")}
        </Button>
      }
    >
      <div className="max-h-[60vh] overflow-y-auto" data-testid="whats-new">
        <MarkdownView markdown={markdown} />
      </div>
    </Dialog>
  );
}

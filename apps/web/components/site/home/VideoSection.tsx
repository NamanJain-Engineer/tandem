import { VIDEO } from "../../../lib/media";
import { Section } from "../Section";

/** #demo (§6.14): the product video. It has sound, so it never autoplays and
 * is never muted; it loads nothing until it's played. */
export function VideoSection() {
  return (
    <Section id="demo" tone="raised" eyebrow="See it run" title="Vibezeo in 1 minute">
      <figure className="reveal flex flex-col gap-4">
        <div data-stage="" className="overflow-hidden rounded-lg bg-stage shadow-stage">
          <video
            poster={VIDEO.poster}
            width={1600}
            height={900}
            controls
            playsInline
            preload="none"
            style={{ aspectRatio: "16 / 9" }}
            className="block h-auto w-full"
          >
            {VIDEO.sources.map((s) => (
              <source key={s.src} src={s.src} type={s.type} />
            ))}
          </video>
        </div>
        <figcaption className="max-w-[52rem] text-caption text-pretty text-fg-muted">
          Every shot of the app is a screen recording of the first Linux release, {VIDEO.recordedWith}; the
          changelog lists what each release since has added. The terminal part plays at 2.3× speed (marked on
          screen), and the live-status scene is an illustration. 0:08 A new tab finds Claude Code on the machine.
          0:14 Four terminals over Vibezeo&apos;s own repository: htop, the test suite and vim. 0:34 The app is
          killed with kill -9. 0:36 Relaunched, the layout comes back.
        </figcaption>
      </figure>
    </Section>
  );
}

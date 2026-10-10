# Product video

The 64-second product video on vibezeo.com (`apps/web/public/screens/product.mp4`
and `.webm`), made with [Remotion](https://www.remotion.dev/). It isn't part of
the pnpm workspace: install and render it on its own.

```bash
cd media/product-video
npm ci
node audio/music.mjs public/soundtrack.wav     # synthesizes the soundtrack (no samples)
BROWSER=/opt/pw-browsers/chromium node render.mjs out/vibezeo-product.mp4
BROWSER=/opt/pw-browsers/chromium node stills.mjs 30 160 300   # preview frames
```

- `src/`: the composition (`Video.tsx` cuts on the soundtrack's bar lines; `scenes.tsx` holds the scenes).
- `public/clips/app.mp4`: one continuous take of the real released app, recorded by
  `capture/record.sh` with `capture/vz.sh` (a 2x Xvfb display, xdotool, ffmpeg, the
  AppImage from `bash scripts/release-local.sh --dry-run`). Re-record it after UI changes.
- Honesty rules for the video: every app shot is the released build; sped-up footage
  carries an on-screen speed tag; anything not filmed from the real app is labelled
  "Illustration"; callouts only claim what the footage shows.

Render with nothing else running (it needs about 2 GB of disk for frame caches).
To publish: transcode to `product.mp4` (H.264) and `product.webm` (VP9) in
`apps/web/public/screens/`, then deploy the site.

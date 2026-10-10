const FONT = /\.(woff2?|ttf|otf)$/i;

/** Vite's `build.assetsInlineLimit` for the demo build: fonts always ship
 * as files, because the site's CSP (`font-src 'self'`, apps/web/vercel.json)
 * refuses `data:` fonts. Anything else keeps Vite's default limit. */
export function inlineAsset(file: string): false | undefined {
  return FONT.test(file) ? false : undefined;
}

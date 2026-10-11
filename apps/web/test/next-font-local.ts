// next/font/local only works under Next's compiler; in vitest a font is its
// CSS variable's name, which is all the pages read from it.
type Options = { variable?: string };
export default function localFont(options: Options) {
  const name = (options.variable ?? "--font").replace(/^--/, "");
  return { className: name, variable: name, style: { fontFamily: name } };
}

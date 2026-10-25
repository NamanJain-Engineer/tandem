/** The site's theme choice (F-WB-01 AC4, D-1870): System, Light or Dark.
 * System follows the OS in CSS alone; Light and Dark set `data-theme` on
 * `<html>`, which tokens.css and site.css answer. */
export const THEME_KEY = "vibezeo-theme";

export const THEME_CHOICES = ["system", "light", "dark"] as const;
export type ThemeChoice = (typeof THEME_CHOICES)[number];

/** The browser bar's colour in each theme: the page background. */
export const THEME_COLOR = { dark: "#09090b", light: "#fafafa" } as const;

/** The inline script that runs first in `<head>`, before the body is
 * parsed, so no page paints in the wrong theme (D-1871). It reads the choice
 * (storage may be blocked: every read and write is wrapped), sets
 * `data-theme-choice`, `data-theme` and `color-scheme`, and answers the
 * header's radios through one delegated listener, so it needs nothing from
 * the page. A change lands without transitions (base.css's
 * `data-theme-switching`, as in the app, D-815). Another tab's choice, and a
 * page restored from the back-forward cache, follow too. It also marks
 * `<html>` with `data-fx`, which turns on the product scenes' effects, and
 * takes it off at `load` unless the scene player (`/demo/embed.js`) has said
 * it runs (`data-scenes`): without the player a scene shows its final frame
 * rather than waiting on its first (D-1880). Written by hand in
 * ES5: it's inlined in every page and must parse everywhere. */
export const THEME_SCRIPT = `(function(){var K=${JSON.stringify(THEME_KEY)},d=document.documentElement,C=${JSON.stringify(THEME_COLOR)};
function ok(v){return v==="light"||v==="dark"}
function read(){try{var v=localStorage.getItem(K);return ok(v)?v:"system"}catch(e){return "system"}}
function sync(c){var i,r=document.querySelectorAll("input[data-theme-set]"),m=document.querySelectorAll('meta[name="theme-color"]');
for(i=0;i<r.length;i++)r[i].checked=r[i].value===c;
for(i=0;i<m.length;i++){if(!m[i].hasAttribute("data-content"))m[i].setAttribute("data-content",m[i].content);m[i].content=ok(c)?C[c]:m[i].getAttribute("data-content")}}
function apply(c,quiet){if(!quiet&&d.getAttribute("data-theme-choice")!==c){d.setAttribute("data-theme-switching","");requestAnimationFrame(function(){requestAnimationFrame(function(){d.removeAttribute("data-theme-switching")})})}
d.setAttribute("data-theme-choice",c);if(ok(c)){d.setAttribute("data-theme",c);d.style.colorScheme=c}else{d.removeAttribute("data-theme");d.style.colorScheme=""}sync(c)}
apply(read(),true);
d.setAttribute("data-fx","");window.addEventListener("load",function(){if(!d.hasAttribute("data-scenes"))d.removeAttribute("data-fx")});
document.addEventListener("change",function(e){var t=e.target;if(!t||!t.hasAttribute||!t.hasAttribute("data-theme-set"))return;var c=ok(t.value)?t.value:"system";
try{if(c==="system")localStorage.removeItem(K);else localStorage.setItem(K,c)}catch(x){}apply(c)});
window.addEventListener("storage",function(e){if(e.key===K||e.key===null)apply(read())});
window.addEventListener("pageshow",function(e){if(e.persisted)apply(read(),true)});
if(document.readyState==="loading")document.addEventListener("DOMContentLoaded",function(){sync(d.getAttribute("data-theme-choice"))})})();`;

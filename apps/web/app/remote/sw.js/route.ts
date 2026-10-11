// The phone companion's service worker (F-RM-03): it keeps the app shell so
// the installed app opens without a connection. It never caches API or relay
// traffic; frames go over the WebSocket, which service workers don't see.
const WORKER = `
const CACHE = "remote-shell-v1";
self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(CACHE).then((c) => c.add("/remote")).then(() => self.skipWaiting()));
});
self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});
self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (event.request.mode !== "navigate" || url.origin !== self.location.origin || !url.pathname.startsWith("/remote")) return;
  event.respondWith(
    fetch(event.request)
      .then((res) => {
        const copy = res.clone();
        caches.open(CACHE).then((c) => c.put("/remote", copy));
        return res;
      })
      .catch(() => caches.match("/remote")),
  );
});
`;

export function GET() {
  return new Response(WORKER, {
    headers: {
      "content-type": "text/javascript; charset=utf-8",
      "service-worker-allowed": "/remote",
      "cache-control": "no-cache",
    },
  });
}

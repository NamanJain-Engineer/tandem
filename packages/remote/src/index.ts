// The phone companion's shared code (F-RM-03, ARCHITECTURE §13.3): the wire
// protocol, the phone's client and its pairing store. The phone web app
// (`apps/web/app/remote`), the LAN web app the desktop serves
// (`apps/web/remote-lan`) and Vibezeo Remote (`apps/mobile`) all use it.
export * from "./protocol";
export * from "./client";
export * from "./store";
export * from "./session";
export * from "./demo";
export * from "./push";
export type * from "./messages";

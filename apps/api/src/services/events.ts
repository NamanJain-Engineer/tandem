import type { Redis } from "ioredis";

export type MeEventKind = "entitlement" | "credits";
type Listener = (kind: MeEventKind) => void;

/**
 * Per-user invalidations over Redis pub/sub (`me:<userId>`), fanned out to SSE
 * streams in this process (ARCHITECTURE §14.4).
 */
export class EventHub {
  private readonly listeners = new Map<string, Set<Listener>>();
  private subscribed = false;

  constructor(
    private readonly pub: Redis,
    private readonly sub: Redis,
  ) {}

  async publish(userId: string, kind: MeEventKind): Promise<void> {
    await this.pub.publish(`me:${userId}`, kind);
  }

  private async ensureSubscribed(): Promise<void> {
    if (this.subscribed) return;
    this.subscribed = true;
    await this.sub.psubscribe("me:*");
    this.sub.on("pmessage", (_pattern: string, channel: string, message: string) => {
      const userId = channel.slice(3);
      for (const listener of this.listeners.get(userId) ?? []) listener(message as MeEventKind);
    });
  }

  async listen(userId: string, listener: Listener): Promise<() => void> {
    await this.ensureSubscribed();
    let set = this.listeners.get(userId);
    if (!set) {
      set = new Set();
      this.listeners.set(userId, set);
    }
    set.add(listener);
    const own = set;
    return () => {
      own.delete(listener);
      // A stream can stop twice (its abort, then its heartbeat loop up to 25 s
      // later): by then a new stream may own the user's entry (D-865).
      if (own.size === 0 && this.listeners.get(userId) === own) this.listeners.delete(userId);
    };
  }

  async close(): Promise<void> {
    this.listeners.clear();
    if (this.subscribed) await this.sub.punsubscribe("me:*").catch(() => undefined);
  }
}

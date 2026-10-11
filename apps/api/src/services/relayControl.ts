/**
 * Lets routes tell the relay to drop a pairing's sockets (F-BE-11, revoke).
 * The relay runs in the API process (D-1903), so this stays in memory.
 */
export class RelayControl {
  private readonly listeners = new Set<(pairingId: string) => void>();

  onDrop(listener: (pairingId: string) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  drop(pairingId: string): void {
    for (const listener of this.listeners) listener(pairingId);
  }
}

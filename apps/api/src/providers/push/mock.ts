import type { PushMessage, PushProvider, PushResult } from "./types";

/** Dev and tests: records what would have been sent. Tokens starting `dead` read as unregistered. */
export class MockPush implements PushProvider {
  readonly name = "mock";
  readonly sent: PushMessage[] = [];

  async send(message: PushMessage): Promise<PushResult> {
    if (message.token.startsWith("dead")) return "unregistered";
    this.sent.push(message);
    return "sent";
  }
}

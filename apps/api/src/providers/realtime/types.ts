export type MintRequest = {
  /** The talker's base instructions; the app adds persona, tone and a workspace summary once connected. */
  instructions: string;
  /** A provider voice. A provider falls back to its default for one it doesn't have. */
  voice?: string;
  maxSeconds: number;
};

export type Minted = {
  /** Short-lived and good only for opening the socket. Never logged or stored. */
  token: string;
  wsUrl: string;
  /** The provider's id for the session, kept to ask for its usage later. */
  providerSessionId?: string;
};

/**
 * The realtime speech model behind a Conductor call (F-BE-08, ARCHITECTURE
 * §14.8). The API only mints: the app connects to `wsUrl` itself, so audio
 * and transcripts never pass through the API.
 */
export interface RealtimeProvider {
  readonly name: "mock" | "openai";
  mint(req: MintRequest): Promise<Minted>;
  /** The seconds the provider metered for a session, or null when it doesn't report usage. */
  reportedSeconds(providerSessionId: string): Promise<number | null>;
}

/** The provider couldn't mint: unreachable, refused or misconfigured. Carries the status only. */
export class RealtimeUnavailableError extends Error {
  constructor(
    message: string,
    public readonly status?: number,
  ) {
    super(message);
    this.name = "RealtimeUnavailableError";
  }
}

/** The talker's one tool (ARCHITECTURE §12.6): everything about the app goes to the doer. */
export const ACT_TOOL = {
  name: "act",
  description:
    "Carry out anything about the Vibezeo app: agents, panes, threads, teammates, tasks, settings, finding work. Pass the user's request in their own words. Say what it returns.",
  parameters: {
    type: "object",
    properties: {
      request: { type: "string", description: "What the user wants done, in their own words." },
    },
    required: ["request"],
    additionalProperties: false,
  },
} as const;

import type { Context } from "hono";
import type { ClientInfo } from "./requestContext";

export type AuthedUser = { id: string; via: "bearer" | "cookie"; sessionFamily?: string };

export type AppVariables = {
  requestId: string;
  client: ClientInfo;
  user: AuthedUser | null;
  bearerPresent: boolean;
};

export type AppEnv = { Variables: AppVariables };
export type AppContext = Context<AppEnv>;

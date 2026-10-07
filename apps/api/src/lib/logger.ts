import pino from "pino";

/** Structured logs: ids, routes, statuses and durations. Never bodies. */
export const logger = pino({
  level: process.env.LOG_LEVEL ?? "info",
  redact: {
    paths: [
      "headers.authorization",
      "headers.cookie",
      "headers['set-cookie']",
      "*.password",
      "*.token",
      "*.secret",
      "*.refreshToken",
      "*.accessToken",
    ],
    censor: "[REDACTED]",
  },
});

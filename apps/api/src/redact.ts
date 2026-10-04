const SECRET =
  /\b(?:ghp_[A-Za-z0-9]{20,}|ghu_[A-Za-z0-9]{20,}|sk-[A-Za-z0-9_-]{20,}|sk_(?:live|test)_[A-Za-z0-9]{16,}|AKIA[0-9A-Z]{16})\b/g;

export function redactText(input: string): string {
  return input.replace(SECRET, "[REDACTED]");
}

const SENSITIVE_HEADERS = new Set(["authorization", "cookie", "x-api-key"]);

export function redactHeaders(headers: Headers): Record<string, string> {
  const out: Record<string, string> = {};
  headers.forEach((value, key) => {
    out[key] = SENSITIVE_HEADERS.has(key.toLowerCase()) ? "[REDACTED]" : redactText(value);
  });
  return out;
}

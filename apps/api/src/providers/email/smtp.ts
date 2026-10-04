import nodemailer from "nodemailer";
import type { EmailMessage, EmailProvider } from "./types";

/** SMTP delivery. In dev, SMTP_URL points at mailpit (:1025). */
export class SmtpEmail implements EmailProvider {
  private readonly transport;

  constructor(
    url: string,
    private readonly from: string,
  ) {
    this.transport = nodemailer.createTransport(url);
  }

  async send(message: EmailMessage): Promise<void> {
    await this.transport.sendMail({ from: this.from, ...message });
  }
}

/** Captures mail in memory (unit tests). */
export class MemoryEmail implements EmailProvider {
  readonly sent: EmailMessage[] = [];

  async send(message: EmailMessage): Promise<void> {
    this.sent.push(message);
  }

  latest(to: string): EmailMessage | undefined {
    return [...this.sent].reverse().find((m) => m.to === to);
  }
}

import type { ReactNode } from "react";
import Link from "next/link";
import { ThemeSwitch } from "./site/ThemeSwitch";

// Card and Notice have no Next.js imports, so the LAN web app can use them.
export { Card, Notice } from "./Panel";

/** Page frame for the account portal. */
export function Shell({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle?: string;
  children: ReactNode;
}) {
  return (
    <div className="min-h-screen bg-bg text-fg">
      <header className="border-b border-border">
        <nav className="mx-auto flex h-14 max-w-4xl items-center gap-6 px-6 text-sm">
          <Link href="/" className="flex items-center gap-2 font-semibold text-fg">
            <svg width="20" height="20" viewBox="0 0 28 28" fill="none" aria-hidden="true">
              <circle cx="10.5" cy="14" r="7" stroke="var(--tn-accent)" strokeWidth="2.2" />
              <circle cx="17.5" cy="14" r="7" stroke="var(--tn-accent)" strokeWidth="2.2" />
            </svg>
            Tandem
          </Link>
          <Link href="/account" className="text-fg-muted hover:text-fg">
            Account
          </Link>
          <Link href="/billing" className="text-fg-muted hover:text-fg">
            Billing
          </Link>
          <Link href="/credits" className="text-fg-muted hover:text-fg">
            Credits
          </Link>
          <Link href="/devices" className="text-fg-muted hover:text-fg">
            Devices
          </Link>
          <Link href="/team" className="text-fg-muted hover:text-fg">
            Team
          </Link>
          <Link href="/transparency" className="text-fg-muted hover:text-fg">
            Transparency
          </Link>
          <ThemeSwitch name="theme" className="ml-auto" />
        </nav>
      </header>
      <main className="mx-auto max-w-4xl px-6 py-10">
        <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
        {subtitle && <p className="mt-1 text-md text-fg-muted">{subtitle}</p>}
        <div className="mt-8">{children}</div>
      </main>
    </div>
  );
}

export function Field({
  label,
  name,
  type = "text",
  autoComplete,
  defaultValue,
  required = true,
}: {
  label: string;
  name: string;
  type?: string;
  autoComplete?: string;
  defaultValue?: string;
  required?: boolean;
}) {
  return (
    <label className="flex flex-col gap-1.5 text-sm text-fg-muted">
      {label}
      <input
        name={name}
        type={type}
        autoComplete={autoComplete}
        defaultValue={defaultValue}
        required={required}
        className="h-8 rounded-sm border border-border bg-surface-4 px-2.5 text-base text-fg outline-none focus:border-accent focus:shadow-focus"
      />
    </label>
  );
}

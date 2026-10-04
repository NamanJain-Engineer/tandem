import { Card, Shell } from "../../../components/Shell";

export default function BillingSuccessPage() {
  return (
    <Shell title="You're on Pro">
      <Card>
        <p className="text-md text-fg-muted">
          Thanks for upgrading. Your desktop app picks up the change in a few seconds.
        </p>
        <a
          href="tandem://"
          className="mt-4 inline-flex h-8 items-center rounded-sm bg-accent px-3 font-medium text-accent-fg hover:bg-accent-hover"
        >
          Open Tandem
        </a>
      </Card>
    </Shell>
  );
}

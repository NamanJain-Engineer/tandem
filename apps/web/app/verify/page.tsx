import Link from "next/link";
import { Card, Shell } from "../../components/Shell";

export default function VerifyPage() {
  return (
    <Shell title="Your email is verified">
      <Card>
        <p className="text-md text-fg-muted">You're signed in.</p>
        <Link className="mt-4 inline-block text-accent hover:underline" href="/account">
          Go to your account
        </Link>
      </Card>
    </Shell>
  );
}

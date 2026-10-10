import hosted from "@tandem/protocol/hosted-features.json";
import subprocessors from "@tandem/protocol/subprocessors.json";
import { Card, Shell } from "../../components/Shell";

type Feature = {
  id: string;
  label: string;
  sends: string;
  destination: string;
  state: { kind: string; key?: string; equals?: unknown; field?: string };
};

/** When a row's data is sent, in words (the app shows the live On/Off). */
function when(f: Feature): string {
  switch (f.state.kind) {
    case "signed_in":
      return "While you're signed in";
    case "on_use":
      return "Only when you use it";
    case "policy":
      return "Only when your Team admins turn it on";
    default:
      return f.id === "updates" ? "On unless you turn it off" : "Off unless you turn it on";
  }
}

/** F-TL-07 on the portal: what leaves the machine, and who processes it. */
export default function TransparencyPage() {
  const features = hosted.features as Feature[];
  return (
    <Shell
      title="What leaves your machine"
      subtitle="Vibezeo runs your coding agents on your computer. These are the only things it sends anywhere, and when."
    >
      <div className="flex flex-col gap-6">
        <Card>
          <table className="w-full text-left text-sm" data-testid="hosted-features">
            <thead className="text-fg-subtle">
              <tr>
                <th className="py-2 pr-4 font-medium">Feature</th>
                <th className="py-2 pr-4 font-medium">When</th>
                <th className="py-2 pr-4 font-medium">What it sends</th>
                <th className="py-2 font-medium">Where</th>
              </tr>
            </thead>
            <tbody>
              {features.map((f) => (
                <tr key={f.id} className="border-t border-divider">
                  <td className="py-2 pr-4">{f.label}</td>
                  <td className="py-2 pr-4 text-fg-muted">{when(f)}</td>
                  <td className="py-2 pr-4 text-fg-muted">{f.sends}</td>
                  <td className="py-2 text-fg-muted">{f.destination}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="mt-4 text-sm text-fg-muted">
            Prompts, file contents, terminal output and AI-vendor sign-ins never leave your machine through Vibezeo, except the
            text a feature above names. The app shows each row&apos;s live state in Settings › Privacy.
          </p>
        </Card>
        <Card>
          <h2 className="text-lg font-semibold">Subprocessors</h2>
          <table className="mt-3 w-full text-left text-sm" data-testid="subprocessors">
            <tbody>
              {subprocessors.subprocessors.map((s) => (
                <tr key={s.id} className="border-t border-divider first:border-t-0">
                  <td className="py-2 pr-4">{s.name}</td>
                  <td className="py-2 pr-4 text-fg-muted">{s.purpose}</td>
                  <td className="py-2 text-fg-muted">{s.data}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      </div>
    </Shell>
  );
}

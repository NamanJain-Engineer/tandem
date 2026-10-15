import { Check, Minus } from "lucide-react";
import { PLAN_GROUPS, PLANS } from "../../lib/pricing";
import { Icon } from "./Icon";

const IDS = ["free", "pro", "team"] as const;

/** /pricing's comparison (§6.16): every row generated from LIMITS. The plan
 * header sticks under the site header; the table scrolls inside its own box
 * on a phone. */
export function PlanComparison() {
  return (
    <div className="surface relative w-full max-w-full min-w-0 overflow-x-auto">
      <table className="w-full min-w-[40rem] border-collapse text-left text-copy-sm">
        <caption className="sr-only">Plan limits and features, from Vibezeo&apos;s plan definitions</caption>
        <thead>
          <tr>
            <th scope="col" className="w-[34%] p-5 font-medium text-fg-muted">
              <span className="sr-only">Feature</span>
            </th>
            {IDS.map((id) => (
              <th key={id} scope="col" className="p-5 text-title text-fg">
                {PLANS.find((p) => p.id === id)?.name}
              </th>
            ))}
          </tr>
        </thead>
        {PLAN_GROUPS.map((g) => (
          <tbody key={g.title}>
            <tr>
              <th colSpan={4} scope="colgroup" className="border-t border-line bg-(--site-glass) px-5 pb-2.5 pt-5 text-left">
                <span className="font-mono text-eyebrow uppercase tracking-[0.12em] text-accent">{g.title}</span>
                {g.note && <span className="ml-3 text-caption font-normal text-fg-muted">{g.note}</span>}
              </th>
            </tr>
            {g.rows.map((r) => (
              <tr key={r.key} data-limit={r.key} className="border-t border-line transition-colors hover:bg-(--site-glass)">
                <th scope="row" className="px-5 py-3.5 font-normal text-fg">
                  {r.label}
                </th>
                {IDS.map((id) => {
                  const v = r.value(id);
                  return (
                    <td key={id} data-plan={id} className="px-5 py-3.5 text-fg">
                      {v === true ? (
                        <>
                          <Icon icon={Check} size={18} className="text-accent" />
                          <span className="sr-only">Included</span>
                        </>
                      ) : v === false ? (
                        <>
                          <Icon icon={Minus} size={18} className="text-fg-subtle" />
                          <span className="sr-only">Not included</span>
                        </>
                      ) : (
                        <span className="font-mono text-[0.875rem] tabular-nums">{v}</span>
                      )}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        ))}
      </table>
    </div>
  );
}

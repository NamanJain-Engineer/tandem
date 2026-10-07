import { render } from "@testing-library/react";
import { configureAxe } from "vitest-axe";
import { describe, expect, it } from "vitest";
import { stableIds } from "../test/normalize";
import { UiProvider } from "../theme/ThemeProvider";
import { galleryEntries } from "./entries";

// Component-level checks: `region` is a page-level rule (portalled popovers
// sit outside landmarks by design); page-level axe runs in the E2E specs.
const axe = configureAxe({ rules: { region: { enabled: false } } });

// UI_SPEC §0 and §7.6: a snapshot and an axe check for every gallery state,
// in both themes. Portalled overlays are included via document.body.
describe("@tandem/ui gallery", () => {
  for (const entry of galleryEntries) {
    describe(`${entry.title} (UI_SPEC ${entry.spec})`, () => {
      for (const state of entry.states) {
        it(`${state.name}: snapshot`, () => {
          render(
            <UiProvider theme="dark">
              <main>{state.render()}</main>
            </UiProvider>,
          );
          expect(stableIds(document.body.innerHTML)).toMatchSnapshot();
        });
        for (const theme of ["dark", "light"] as const) {
          it(`${state.name}: no axe violations (${theme})`, async () => {
            render(
              <UiProvider theme={theme}>
                <main>{state.render()}</main>
              </UiProvider>,
            );
            expect(document.documentElement.dataset.theme).toBe(theme);
            expect(await axe(document.body)).toHaveNoViolations();
          });
        }
      }
    });
  }
});

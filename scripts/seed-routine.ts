// Seeds a routine into $TANDEM_HOME's database for the headless runner's
// check (MILESTONES M11 verification, F-RT-03 AC2):
//   TANDEM_HOME=$H pnpm tsx scripts/seed-routine.ts --due-now
//   TANDEM_HOME=$H ./target/debug/tandem --run-routines
// Creates the database with the app's own migrations when it isn't there,
// adds a teammate and a background routine, and with --due-now makes it due
// a minute ago. Prints the routine's id.
import { mkdirSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const home = process.env.TANDEM_HOME;
if (!home) {
  console.error("Set TANDEM_HOME to the profile to seed (never your real one).");
  process.exit(2);
}
const dueNow = process.argv.includes("--due-now");
const root = new URL("..", import.meta.url).pathname;

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
function ulid(): string {
  let time = Date.now();
  let out = "";
  for (let i = 0; i < 10; i++) {
    out = CROCKFORD[time % 32] + out;
    time = Math.floor(time / 32);
  }
  for (let i = 0; i < 16; i++) out += CROCKFORD[Math.floor(Math.random() * 32)];
  return out;
}

mkdirSync(home, { recursive: true, mode: 0o700 });
const db = new DatabaseSync(join(home, "tandem.db"));
db.exec("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");
const version = (db.prepare("PRAGMA user_version").get() as { user_version: number }).user_version;
const migrations = readdirSync(join(root, "crates/tandem-store/migrations"))
  .filter((f) => f.endsWith(".sql"))
  .sort();
migrations.slice(version).forEach((file, i) => {
  db.exec("BEGIN");
  db.exec(readFileSync(join(root, "crates/tandem-store/migrations", file), "utf8"));
  db.exec(`PRAGMA user_version = ${version + i + 1}`);
  db.exec("COMMIT");
});

const now = Date.now();
const teammate = `tm_${ulid()}`;
const teammateHome = join(home, "teammates", teammate);
mkdirSync(join(teammateHome, "memory"), { recursive: true, mode: 0o700 });
db.prepare(
  `INSERT INTO teammates (id, name, name_fold, engine_id, home, settings_json, is_lead, pinned, sort_key, created_at)
   VALUES (?, ?, ?, 'claude', ?, ?, 0, 0, 'm', ?)`,
).run(
  teammate,
  `Seeded ${teammate.slice(-4)}`,
  `seeded ${teammate.slice(-4).toLowerCase()}`,
  teammateHome,
  JSON.stringify({ memoryCap: 4000, learnMode: "ask", mayMessage: true, maySchedule: false, defaultAccess: "edits" }),
  now,
);
const routine = `rt_${ulid()}`;
db.prepare(
  `INSERT INTO routines (id, teammate_id, name, instruction, schedule_json, timezone, access, run_mode, next_run_at, created_at, updated_at)
   VALUES (?, ?, 'Seeded routine', 'Say hello in one line.', ?, 'UTC', 'edits', 'background', ?, ?, ?)`,
).run(
  routine,
  teammate,
  JSON.stringify({ kind: "interval", every_minutes: 60 }),
  dueNow ? now - 60_000 : now + 60 * 60_000,
  now,
  now,
);
db.close();
console.log(routine);

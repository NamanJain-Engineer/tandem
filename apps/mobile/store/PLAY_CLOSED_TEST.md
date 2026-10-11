# Google Play closed test (personal developer accounts)

Play requires personal accounts made after 13 November 2023 to run a closed
test with at least 12 testers opted in for 14 days in a row before production.

1. Play Console › Vibezeo Remote › Testing › Closed testing › Create track
   "early-access". Upload `VibezeoRemote-<version>.aab` (signed with your
   upload key).
2. Testers: a Google Group (for example vibezeo-testers@googlegroups.com) with
   at least 15 people, so the count stays at 12 or more if some leave.
3. Share the opt-in link. Ask each tester to install from Play, try the demo,
   and pair once with a desktop on their Wi-Fi (LAN mode).
4. Keep them opted in for 14 full days. Ship at least one update during the
   test (Play looks for an engaged test).
5. Collect feedback in a form: pairing worked (yes/no), crashes, confusing
   screens. Fix, upload, and note the changes.
6. Day 15: Production › Apply for production access, with the test's summary.

"""Frees disk in target/debug: deletes the workspace's stale per-crate build
artifacts (all but the newest `keep` hashes per crate, older than 30 min) and
the incremental dir. Run it before a gate or E2E build when disk is short.
    python3 scripts/agent/prune-target.py [keep=3]"""
import collections, os, re, shutil, sys, time

root = os.path.abspath(os.path.join(os.path.dirname(__file__), "../.."))
keep = int(sys.argv[1]) if len(sys.argv) > 1 else 3
shutil.rmtree(os.path.join(root, "target/debug/incremental"), ignore_errors=True)
d = os.path.join(root, "target/debug/deps")
groups = collections.defaultdict(lambda: collections.defaultdict(list))
for f in os.listdir(d) if os.path.isdir(d) else []:
    m = re.match(r"^(lib)?([a-z0-9_]+)-([0-9a-f]{16})(\..*)?$", f)
    if not m:
        continue
    name = m.group(2)
    if not (name.startswith("tandem") or name in ("fake_engine", "desktop", "app_lib")):
        continue
    p = os.path.join(d, f)
    groups[name][m.group(3)].append((os.path.getmtime(p), p))
freed = 0
for name, by_hash in groups.items():
    hashes = sorted(by_hash, key=lambda h: max(t for t, _ in by_hash[h]), reverse=True)
    for h in hashes[keep:]:
        for t, p in by_hash[h]:
            if time.time() - t > 1800 and os.path.isfile(p):
                freed += os.path.getsize(p)
                os.remove(p)
print("pruned GB", round(freed / 1e9, 2))

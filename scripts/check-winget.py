"""Validates winget manifests against winget's JSON schemas (D-1715).

    python3 -I scripts/check-winget.py <manifest dir> <schema dir>

Needs PyYAML and jsonschema (pip install pyyaml jsonschema).
"""
import json
import pathlib
import sys

import jsonschema
import yaml

class Loader(yaml.SafeLoader):
    """YAML as winget reads it: a date stays a string."""


Loader.yaml_implicit_resolvers = {
    ch: [(tag, rx) for tag, rx in rs if tag != "tag:yaml.org,2002:timestamp"]
    for ch, rs in yaml.SafeLoader.yaml_implicit_resolvers.items()
}

KINDS = {"version": "version", "installer": "installer", "defaultLocale": "defaultLocale"}


def main() -> int:
    manifests, schemas = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
    seen = set()
    failed = False
    for path in sorted(manifests.glob("*.yaml")):
        doc = yaml.load(path.read_text(), Loader=Loader)
        kind = doc.get("ManifestType")
        if kind not in KINDS:
            print(f"check-winget: {path.name}: unknown ManifestType {kind!r}", file=sys.stderr)
            failed = True
            continue
        seen.add(kind)
        schema = json.loads((schemas / f"manifest.{KINDS[kind]}.1.9.0.json").read_text())
        errors = sorted(jsonschema.Draft7Validator(schema).iter_errors(doc), key=str)
        for e in errors:
            print(f"check-winget: {path.name}: {'/'.join(map(str, e.path))}: {e.message}", file=sys.stderr)
        failed |= bool(errors)
    missing = set(KINDS) - seen
    if missing:
        print(f"check-winget: missing {sorted(missing)}", file=sys.stderr)
        failed = True
    ids = {yaml.load(p.read_text(), Loader=Loader)["PackageVersion"] for p in manifests.glob("*.yaml")}
    if len(ids) != 1:
        print(f"check-winget: versions disagree: {sorted(ids)}", file=sys.stderr)
        failed = True
    if not failed:
        print(f"check-winget: {manifests} ok")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

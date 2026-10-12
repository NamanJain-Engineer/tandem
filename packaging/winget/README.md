# winget

`scripts/gen-winget.sh` writes the three manifests (version, installer,
default locale; schema 1.9.0) for one release into
`dist/winget/manifests/v/Vibezeo/Vibezeo/<version>/` and validates them
against the JSON schemas here, copied from microsoft/winget-cli (MIT,
`schemas/JSON/manifests/v1.9.0`). Submitting them is a pull request to
microsoft/winget-pkgs (HUMAN_TODO, D-1715).

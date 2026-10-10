# tandem-host binaries

The helper Vibezeo uploads to SSH hosts (F-RM-01). Not committed:
`scripts/build-tandem-host.sh` builds `linux-x86_64/tandem-host` here and
writes `manifest.json` (SHA-256 per binary); CI and the release workflow
build all three platforms (`scripts/ci/build-tandem-host.sh`). A build
without them says so when you connect to a host.

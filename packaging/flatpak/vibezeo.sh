#!/bin/sh
# Starts Vibezeo inside the Flatpak. The app's resources (helpers, the LAN
# phone web app) sit beside the binary.
exec /app/lib/vibezeo/tandem "$@"

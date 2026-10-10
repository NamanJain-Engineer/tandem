#!/bin/sh
# Vercel's Ignored Build Step (vercel.json `ignoreCommand`, D-809): exit 0
# skips the deployment, exit 1 builds it. Vercel runs it in apps/web, the
# project's Root Directory, on a shallow clone. It lives in this file because
# Vercel caps the command itself at 256 characters.

# Only main (production), product-m0-m5 and release-* (previews) deploy.
case "$VERCEL_GIT_COMMIT_REF" in
  main | product-m0-m5 | release-*) ;;
  *)
    echo "Only main, product-m0-m5 and release-* deploy (docs/DEPLOY_SITE.md)"
    exit 0
    ;;
esac

# Skip when nothing the site uses changed since the last deployment. When
# that commit isn't in the shallow clone (after a large merge), build rather
# than fail on "bad object".
prev="$VERCEL_GIT_PREVIOUS_SHA"
if [ -n "$prev" ] && git cat-file -e "$prev^{commit}" 2>/dev/null &&
  git diff --quiet "$prev" HEAD -- . ../desktop/src ../desktop/demo.html ../desktop/vite.demo.config.ts \
    ../desktop/package.json ../../packages/ui ../../packages/protocol ../../pnpm-lock.yaml ../../CHANGELOG.md ../../crates/tandem-engines/registry.json; then
  echo "Nothing the site uses changed since the last deployment"
  exit 0
fi
exit 1

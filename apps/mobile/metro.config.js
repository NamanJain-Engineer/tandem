// Metro in the pnpm workspace: watch the shared packages (@tandem/remote,
// @tandem/protocol) and resolve from this app's node_modules first.
const path = require("path");
const { getDefaultConfig } = require("expo/metro-config");

const root = path.resolve(__dirname, "../..");
const config = getDefaultConfig(__dirname);
config.watchFolders = [path.join(root, "packages")];
config.resolver.nodeModulesPaths = [path.join(__dirname, "node_modules"), path.join(root, "node_modules")];
config.resolver.unstable_enableSymlinks = true;
config.resolver.unstable_enablePackageExports = true;
module.exports = config;

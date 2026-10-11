// What the shared protocol (@tandem/remote, @noble/*) needs from the
// JavaScript runtime that Hermes may not have: a CSPRNG behind
// `crypto.getRandomValues` (expo-crypto: SecRandomCopyBytes on iOS,
// SecureRandom on Android) and a UTF-8 `TextDecoder`.
import { getRandomValues } from "expo-crypto";
import { installPolyfills } from "./runtime";

type Install = Parameters<typeof installPolyfills>;
installPolyfills(globalThis as unknown as Install[0], getRandomValues as unknown as Install[1]);

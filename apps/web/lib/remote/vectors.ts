// Builds the protocol's interop vectors from fixed inputs. The committed
// copy is crates/tandem-core/tests/fixtures/remote_v1.json; the Rust and
// TypeScript tests both check their bytes against it (D-1905).
import {
  b64url,
  encodeHello,
  epochKeys,
  fromB64url,
  helloTag,
  nextSecret,
  publicKeyOf,
  rootSecret,
  seal,
} from "./protocol";

const hex = (h: string) => Uint8Array.from(h.match(/../g) ?? [], (b) => parseInt(b, 16));
const fill = (n: number, v: number) => new Uint8Array(n).fill(v);

export const VECTOR_INPUTS = {
  pid: "pr_01JTESTVECTOR0000000000000",
  desktopSecret: b64url(hex("a8abababababababababababababababababababababababababababababab6b")),
  phoneSecret: b64url(hex("c8cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd4d")),
  s: b64url(fill(32, 0x5a)),
  desktopEph: b64url(hex("10111111111111111111111111111111111111111111111111111111111111d1")),
  phoneEph: b64url(hex("20222222222222222222222222222222222222222222222222222222222222e2")),
  random16: b64url(fill(16, 0x7e)),
  plaintextD2p: '{"t":"welcome","desktop":"Test desktop"}',
  plaintextP2d: '{"t":"cmd","id":1,"cmd":"view.snapshot","args":{}}',
};

export function buildVectors(inputs = VECTOR_INPUTS) {
  const enc = new TextEncoder();
  const d = fromB64url(inputs.desktopSecret);
  const p = fromB64url(inputs.phoneSecret);
  const s = fromB64url(inputs.s);
  const dpk = publicKeyOf(d);
  const ppk = publicKeyOf(p);
  const tag = helloTag(s, inputs.pid, ppk);
  const root = rootSecret(s, inputs.pid, dpk, ppk, d, ppk);
  const e0 = epochKeys(0, root);
  const r = fromB64url(inputs.random16);
  const d2p = seal(inputs.pid, e0.d2p, { epoch: 0, dir: "d2p", ctr: 1n }, enc.encode(inputs.plaintextD2p), r);
  const p2d = seal(inputs.pid, e0.p2d, { epoch: 0, dir: "p2d", ctr: 1n }, enc.encode(inputs.plaintextP2d), r);
  const de = fromB64url(inputs.desktopEph);
  const pe = fromB64url(inputs.phoneEph);
  const e1 = epochKeys(1, nextSecret(e0, de, publicKeyOf(pe)));
  const d2pE1 = seal(inputs.pid, e1.d2p, { epoch: 1, dir: "d2p", ctr: 7n }, enc.encode(inputs.plaintextD2p), r);
  return {
    inputs,
    desktopPublic: b64url(dpk),
    phonePublic: b64url(ppk),
    desktopEphPublic: b64url(publicKeyOf(de)),
    phoneEphPublic: b64url(publicKeyOf(pe)),
    helloTag: b64url(tag),
    hello: b64url(encodeHello(ppk, tag)),
    root: b64url(root),
    epoch0: { d2p: b64url(e0.d2p), p2d: b64url(e0.p2d) },
    sealedD2pCtr1: b64url(d2p),
    sealedP2dCtr1: b64url(p2d),
    epoch1: { secret: b64url(e1.secret), d2p: b64url(e1.d2p), p2d: b64url(e1.p2d) },
    sealedD2pEpoch1Ctr7: b64url(d2pE1),
  };
}

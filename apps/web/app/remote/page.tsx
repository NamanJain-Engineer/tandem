import { SITE } from "../../lib/site";
import { RemoteApp } from "./RemoteApp";

/** F-RM-03: the phone companion PWA (ARCHITECTURE §13.3, §15). */
export default function RemotePage() {
  return <RemoteApp product={SITE.product} />;
}

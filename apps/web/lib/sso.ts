/** Why a single sign-on came back without signing in (F-BL-06 P2). */
export const SSO_ERRORS: Record<string, string> = {
  SSO_STATE: "That sign-in link expired. Start again.",
  SSO_DENIED: "Your identity provider didn't sign you in.",
  SSO_DOMAIN: "Your identity provider signed in an address outside your organization's verified domains.",
  SSO_TOKEN: "Your identity provider's answer couldn't be verified. Ask your admin to check the SSO settings.",
  SSO_EXCHANGE: "Your identity provider didn't accept the sign-in. Ask your admin to check the SSO settings.",
  SSO_CONFIG: "Your organization's SSO settings couldn't be read. Ask your admin to check them.",
  SEATS_FULL: "Your organization has no free seat. Ask an admin to add one.",
  SSO_OTHER_ORG: "This account already belongs to another organization.",
  NO_SSO: "Single sign-on isn't set up for that address any more.",
  SSO_ACCOUNT: "Couldn't sign you in. Try again.",
};

// The messages inside sealed frames, generated from the desktop's Rust types
// (`tandem_core::remote::messages`, `services::phone`) by ts-rs.
export type {
  FromPhone,
  PhoneAgent,
  PhoneApproval,
  PhoneDecision,
  PhoneGrants,
  PhoneSnapshot,
  PhoneThread,
  PhoneTranscript,
  PhoneTranscriptItem,
  ResError,
  ToPhone,
} from "@tandem/protocol";

/** ts-rs's `serde_json::Value`, which the package doesn't export by name. */
export type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null;

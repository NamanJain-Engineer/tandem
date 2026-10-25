-- F-TL-12: the names of Settings › Secrets. The values live in the keychain
-- (NS.secrets/<name>), never here.
CREATE TABLE secrets (
  name       TEXT PRIMARY KEY,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- F-EN-11 (D-2519): a pane started on a local model keeps "{model} · {kind}",
-- which its header shows as "On this computer".
ALTER TABLE panes ADD COLUMN local_model TEXT;

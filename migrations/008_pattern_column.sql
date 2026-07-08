-- Track which launch pattern (team | executor | simple | fixbug) a session
-- was started with. Defaults to 'team' so existing rows and any writer that
-- hasn't been updated yet keep behaving exactly as before.
ALTER TABLE sessions ADD COLUMN pattern TEXT NOT NULL DEFAULT 'team';

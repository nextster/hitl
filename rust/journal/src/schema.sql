
CREATE TABLE IF NOT EXISTS events (
 id INTEGER PRIMARY KEY, at REAL NOT NULL, process TEXT NOT NULL, seq INTEGER NOT NULL, identity TEXT UNIQUE,
 actor TEXT NOT NULL, source TEXT NOT NULL, kind TEXT NOT NULL, phase TEXT NOT NULL,
 trace TEXT NOT NULL, action TEXT NOT NULL, parent TEXT NOT NULL,
 request TEXT NOT NULL, attempt TEXT NOT NULL, subject TEXT NOT NULL,
 outcome TEXT NOT NULL, error TEXT NOT NULL, duration_ms REAL,
 input_tokens INTEGER, output_tokens INTEGER, cost_nano_usd INTEGER, usage_known INTEGER,
 context TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS events_trace ON events(trace,id);
CREATE INDEX IF NOT EXISTS events_request ON events(request,id);
CREATE INDEX IF NOT EXISTS events_action ON events(action,phase);
CREATE INDEX IF NOT EXISTS events_at ON events(at);
CREATE TABLE IF NOT EXISTS meta (name TEXT PRIMARY KEY, value INTEGER NOT NULL);
PRAGMA user_version=1;

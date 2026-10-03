-- Inline tests use transient capacity, never submissions or learner attempts.
-- Committed leases share admission with queued submissions across API processes.
CREATE TABLE challenge_coding_inline_runs (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL,
    subtask_id uuid NOT NULL REFERENCES challenges_subtasks(id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL
);
CREATE INDEX coding_inline_user ON challenge_coding_inline_runs(user_id, expires_at);
CREATE INDEX coding_inline_expiry ON challenge_coding_inline_runs(expires_at);

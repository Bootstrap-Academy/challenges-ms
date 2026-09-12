-- Only new submissions use outcome-based charging. Existing queued submissions
-- were charged at admission and must never acquire a second debit on restart.
ALTER TABLE challenges_coding_challenge_submissions
 ADD COLUMN charge_on_failure boolean NOT NULL DEFAULT false;

CREATE TABLE challenge_heart_operations (
 id uuid PRIMARY KEY,
 user_id uuid NOT NULL,
 subtask_id uuid NOT NULL REFERENCES challenges_subtasks(id) ON DELETE CASCADE,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 state text NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','settled','review')),
 attempts integer NOT NULL DEFAULT 0,
 next_attempt_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 receipt jsonb
);
CREATE INDEX challenge_heart_due ON challenge_heart_operations(next_attempt_at)
 WHERE state='pending';
CREATE INDEX challenge_heart_user ON challenge_heart_operations(user_id);

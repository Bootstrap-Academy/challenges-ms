-- Submissions remain the durable unit of work, with their original IDs/content.
ALTER TABLE challenges_coding_challenge_submissions
    ADD COLUMN judge_pending boolean NOT NULL DEFAULT true,
    ADD COLUMN judge_generation bigint NOT NULL DEFAULT 0,
    ADD COLUMN judge_lease_owner uuid,
    ADD COLUMN judge_lease_until timestamptz,
    ADD COLUMN judge_available_at timestamptz NOT NULL DEFAULT now();

UPDATE challenges_coding_challenge_submissions s SET judge_pending = false
WHERE EXISTS (SELECT 1 FROM challenges_coding_challenge_result r WHERE r.submission_id = s.id);

CREATE INDEX coding_execution_pending ON challenges_coding_challenge_submissions
    (judge_available_at, creation_timestamp, id) WHERE judge_pending;
CREATE INDEX coding_execution_pending_user ON challenges_coding_challenge_submissions
    (creator) WHERE judge_pending;

-- Expiring capacity advertisement for the existing admin queue response.
CREATE TABLE challenge_coding_workers (
    id uuid PRIMARY KEY,
    capacity integer NOT NULL CHECK (capacity > 0),
    lease_until timestamptz NOT NULL
);

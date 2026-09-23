-- One XP milestone per user and completed skills-ms lesson unit. The row id is
-- the subtask key of its benefit earning, so the existing outbox delivers the XP.
CREATE TABLE challenge_lesson_milestones (
 id uuid PRIMARY KEY,
 user_id uuid NOT NULL,
 unit_id text NOT NULL CHECK(unit_id ~ '^[a-z0-9][a-z0-9-]{0,79}$'),
 skill_id text NOT NULL CHECK(length(skill_id) BETWEEN 1 AND 256),
 xp bigint NOT NULL CHECK(xp > 0),
 completion text NOT NULL CHECK(completion IN ('deterministic','llm_verdict')),
 completed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 UNIQUE(user_id,unit_id)
);

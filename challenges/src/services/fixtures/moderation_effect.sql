-- Entirely synthetic. Eight target states meet every hold scenario once, so
-- each rule of moderation_effect is exercised with and without each of the
-- others. Scenario 0 is a target without any hold.

-- The clock of this fixture stands still. pg_catalog is named last in the
-- search path of this transaction, so every unqualified clock_timestamp()
-- resolves to the function below: in this file, in moderation_effect and in
-- the projection under test. A hold can therefore start or end at exactly the
-- instant which both sides compare with, and a statement which read any other
-- clock would compare with a different time.
CREATE SCHEMA moderation_effect_fixture;
CREATE FUNCTION moderation_effect_fixture.clock_timestamp() RETURNS timestamptz
    LANGUAGE sql AS $$ SELECT timestamptz '2100-01-01 00:00:00+00' $$;
SET LOCAL search_path = moderation_effect_fixture, public, pg_catalog;

CREATE TEMP TABLE moderation_effect_fixture_holds ON COMMIT DROP AS
SELECT * FROM (VALUES
    -- One hold in force and without a stored end, for each effect.
    ( 1,  1, 'hide',     true,  interval '-1 day',  NULL::interval),
    ( 2,  2, 'remove',   true,  interval '-1 day',  NULL),
    ( 3,  3, 'restrict', true,  interval '-1 day',  NULL),
    ( 4,  4, 'retire',   true,  interval '-1 day',  NULL),
    -- One hold in force whose stored end lies ahead.
    ( 5,  5, 'hide',     true,  interval '-1 day',  interval '1 day'),
    ( 6,  6, 'remove',   true,  interval '-1 day',  interval '1 day'),
    ( 7,  7, 'restrict', true,  interval '-1 day',  interval '1 day'),
    ( 8,  8, 'retire',   true,  interval '-1 day',  interval '1 day'),
    -- A hold which was switched off decides nothing.
    ( 9,  9, 'hide',     false, interval '-1 day',  NULL),
    (10, 10, 'remove',   false, interval '-1 day',  NULL),
    (11, 11, 'restrict', false, interval '-1 day',  NULL),
    (12, 12, 'retire',   false, interval '-1 day',  NULL),
    -- A hold which starts later decides nothing yet.
    (13, 13, 'hide',     true,  interval '1 day',   NULL),
    (14, 14, 'remove',   true,  interval '1 day',   interval '2 days'),
    (15, 15, 'restrict', true,  interval '1 day',   NULL),
    (16, 16, 'retire',   true,  interval '1 day',   interval '2 days'),
    -- A hold whose stored end has passed decides nothing any more, even
    -- while the worker has not switched it off.
    (17, 17, 'hide',     true,  interval '-2 days', interval '-1 day'),
    (18, 18, 'remove',   true,  interval '-2 days', interval '-1 day'),
    (19, 19, 'restrict', true,  interval '-2 days', interval '-1 day'),
    (20, 20, 'retire',   true,  interval '-2 days', interval '-1 day'),
    -- Several holds at one target.
    (21, 21, 'hide',     true,  interval '-1 day',  NULL),
    (22, 21, 'retire',   true,  interval '-1 day',  NULL),
    (23, 22, 'remove',   true,  interval '-1 day',  interval '1 day'),
    (24, 22, 'retire',   true,  interval '-1 day',  NULL),
    (25, 23, 'hide',     true,  interval '-2 days', interval '-1 day'),
    (26, 23, 'retire',   true,  interval '-1 day',  NULL),
    (27, 24, 'remove',   false, interval '-1 day',  NULL),
    (28, 24, 'hide',     true,  interval '-1 day',  NULL),
    (29, 25, 'retire',   true,  interval '1 day',   NULL),
    (30, 25, 'restrict', true,  interval '-1 day',  NULL),
    (31, 26, 'hide',     true,  interval '-1 day',  NULL),
    (32, 26, 'remove',   true,  interval '-1 day',  NULL),
    (33, 26, 'restrict', true,  interval '-1 day',  NULL),
    (34, 26, 'retire',   true,  interval '-1 day',  NULL),
    (35, 27, 'remove',   true,  interval '-2 days', interval '-1 day'),
    (36, 27, 'retire',   false, interval '-1 day',  NULL),
    (37, 27, 'hide',     true,  interval '1 day',   NULL),
    (38, 28, 'hide',     true,  interval '-1 day',  NULL),
    (39, 28, 'hide',     true,  interval '-1 day',  interval '1 day'),
    -- The two edges of a hold. It is in force from the very instant of its
    -- start, and it is no longer in force at the instant of its stored end.
    (40, 29, 'hide',     true,  interval '0',             NULL),
    (41, 30, 'retire',   true,  interval '1 microsecond', NULL),
    (42, 31, 'remove',   true,  interval '-1 day',        interval '0'),
    (43, 32, 'restrict', true,  interval '-1 day',        interval '1 microsecond')
) AS holds(hold, scenario, effect, active, starts_in, ends_in);

INSERT INTO moderation_targets (kind, id, subject, base_enabled, base_retired, withdrawn)
SELECT 'subtask',
       ('00000000-0000-4000-8' || lpad(state::text, 3, '0') || '-' || lpad(scenario::text, 12, '0'))::uuid,
       '00000000-0000-4000-a000-000000000001',
       state & 1 = 0, state & 2 <> 0, state & 4 <> 0
FROM generate_series(0, 7) AS state, generate_series(0, 32) AS scenario;

INSERT INTO moderation_cases (id, target_kind, target_id, subject, source, private_evidence)
SELECT ('00000000-0000-4000-9' || lpad(state::text, 3, '0') || '-' || lpad(hold::text, 12, '0'))::uuid,
       'subtask',
       ('00000000-0000-4000-8' || lpad(state::text, 3, '0') || '-' || lpad(scenario::text, 12, '0'))::uuid,
       '00000000-0000-4000-a000-000000000001', 'own_review', '{}'
FROM generate_series(0, 7) AS state, moderation_effect_fixture_holds;

INSERT INTO moderation_holds (case_id, target_kind, target_id, effect, starts_at, ends_at, active)
SELECT ('00000000-0000-4000-9' || lpad(state::text, 3, '0') || '-' || lpad(hold::text, 12, '0'))::uuid,
       'subtask',
       ('00000000-0000-4000-8' || lpad(state::text, 3, '0') || '-' || lpad(scenario::text, 12, '0'))::uuid,
       effect, clock_timestamp() + starts_in, clock_timestamp() + ends_in, active
FROM generate_series(0, 7) AS state, moderation_effect_fixture_holds;

-- A target of another kind may carry the same id. Its holds belong to that
-- other target and must leave the subtask without a hold as it is.
INSERT INTO moderation_targets (kind, id, subject)
SELECT 'create',
       ('00000000-0000-4000-8' || lpad(state::text, 3, '0') || '-000000000000')::uuid,
       '00000000-0000-4000-a000-000000000001'
FROM generate_series(0, 7) AS state;

INSERT INTO moderation_cases (id, target_kind, target_id, subject, source, private_evidence)
SELECT ('00000000-0000-4000-b' || lpad(state::text, 3, '0') || '-' || lpad(hold::text, 12, '0'))::uuid,
       'create',
       ('00000000-0000-4000-8' || lpad(state::text, 3, '0') || '-000000000000')::uuid,
       '00000000-0000-4000-a000-000000000001', 'own_review', '{}'
FROM generate_series(0, 7) AS state, generate_series(1, 3) AS hold;

INSERT INTO moderation_holds (case_id, target_kind, target_id, effect, starts_at)
SELECT ('00000000-0000-4000-b' || lpad(state::text, 3, '0') || '-' || lpad(hold::text, 12, '0'))::uuid,
       'create',
       ('00000000-0000-4000-8' || lpad(state::text, 3, '0') || '-000000000000')::uuid,
       (ARRAY['restrict', 'remove', 'retire'])[hold], clock_timestamp() - interval '1 day'
FROM generate_series(0, 7) AS state, generate_series(1, 3) AS hold;

-- An id which is known only as a target of another kind is no subtask target
-- at all, whatever that other target and its hold say.
INSERT INTO moderation_targets (kind, id, subject, base_retired, withdrawn)
VALUES ('report', '00000000-0000-4000-c000-000000000001', '00000000-0000-4000-a000-000000000001', true, true);
INSERT INTO moderation_cases (id, target_kind, target_id, subject, source, private_evidence)
VALUES ('00000000-0000-4000-c000-000000000002', 'report', '00000000-0000-4000-c000-000000000001',
        '00000000-0000-4000-a000-000000000001', 'own_review', '{}');
INSERT INTO moderation_holds (case_id, target_kind, target_id, effect, starts_at)
VALUES ('00000000-0000-4000-c000-000000000002', 'report', '00000000-0000-4000-c000-000000000001',
        'remove', clock_timestamp() - interval '1 day');

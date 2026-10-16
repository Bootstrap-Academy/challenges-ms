-- Entirely synthetic, shaped like the list of a production instance: tens of
-- thousands of subtask targets, a few of them disabled, retired or withdrawn,
-- and holds of every kind, effect and state. Hold k takes its traits from the
-- bytes of md5('realistic-hold-' || k), so every run builds the same data, and
-- pg_temp.realistic_holds(from, to) adds the holds from..to, so the test can
-- grow the table in steps. The clock is the real one. Apart from the infinite
-- bounds, every hold starts and ends at least a day away from the comparison.

INSERT INTO moderation_targets (kind, id, subject, base_enabled, base_retired, withdrawn)
SELECT 'subtask', md5('realistic-subtask-' || n)::uuid, md5('realistic-author-' || n % 61)::uuid,
       n % 23 <> 0, n % 37 = 0, n % 401 = 0
FROM generate_series(1, 40000) AS n;

-- Ids which are known only as a target of another kind.
INSERT INTO moderation_targets (kind, id, subject, base_retired, withdrawn)
SELECT 'report', md5('realistic-report-' || n)::uuid, md5('realistic-author-' || n % 61)::uuid,
       n % 2 = 0, n % 3 = 0
FROM generate_series(1, 100) AS n;

-- What the list asks for, in a mixed order: every subtask target, every
-- seventh of them a second time, ids without any target and the reports.
CREATE TEMP TABLE realistic_list ON COMMIT DROP AS
SELECT row_number() OVER (ORDER BY md5(source || '-' || n)) AS position, id
FROM (
    SELECT 'first', n, md5('realistic-subtask-' || n)::uuid FROM generate_series(1, 40000) AS n
    UNION ALL
    SELECT 'again', n, md5('realistic-subtask-' || n)::uuid FROM generate_series(7, 40000, 7) AS n
    UNION ALL
    SELECT 'unknown', n, md5('realistic-unknown-' || n)::uuid FROM generate_series(1, 100) AS n
    UNION ALL
    SELECT 'report', n, md5('realistic-report-' || n)::uuid FROM generate_series(1, 100) AS n
) AS listed(source, n, id);

-- Four in five holds belong to a subtask, the others to a target of another
-- kind which shares its id with a subtask or with one of the reports. One in
-- seven holds goes to one of twelve subtasks, so some subtasks carry many.
-- Effects: hide and retire three in ten each, remove and restrict two in ten.
-- Three in four holds are active. One in twenty starts in the future and one
-- in fifty at -infinity. Half have no stored end, a fifth end in the future, a
-- quarter ended in the past and the rest end at infinity.
CREATE FUNCTION pg_temp.realistic_batch(p_from int, p_to int)
RETURNS TABLE (b_case uuid, b_kind text, b_target uuid, b_effect text, b_active boolean,
               b_starts timestamptz, b_ends timestamptz)
LANGUAGE sql VOLATILE AS $$
SELECT md5('realistic-case-' || k)::uuid,
       kind,
       CASE WHEN kind = 'subtask' OR get_byte(r, 9) < 128
            THEN md5('realistic-subtask-' || CASE WHEN get_byte(r, 1) < 37 THEN 1 + get_byte(r, 6) % 12
                                                  ELSE 1 + (k::bigint * 7919) % 40000 END)::uuid
            ELSE md5('realistic-report-' || (1 + get_byte(r, 6) % 100))::uuid END,
       CASE WHEN get_byte(r, 2) < 77 THEN 'hide' WHEN get_byte(r, 2) < 154 THEN 'retire'
            WHEN get_byte(r, 2) < 205 THEN 'remove' ELSE 'restrict' END,
       get_byte(r, 3) < 192,
       CASE WHEN get_byte(r, 4) < 13 THEN clock_timestamp() + (1 + get_byte(r, 7) % 30) * interval '1 day'
            WHEN get_byte(r, 4) < 18 THEN '-infinity'
            ELSE clock_timestamp() - (1 + get_byte(r, 7) % 180) * interval '1 day' END,
       CASE WHEN get_byte(r, 5) < 128 THEN NULL
            WHEN get_byte(r, 5) < 179 THEN clock_timestamp() + (1 + get_byte(r, 8) % 60) * interval '1 day'
            WHEN get_byte(r, 5) < 243 THEN clock_timestamp() - (1 + get_byte(r, 8) % 30) * interval '1 day'
            ELSE 'infinity' END
FROM generate_series(p_from, p_to) AS k,
     decode(md5('realistic-hold-' || k), 'hex') AS r,
     LATERAL (SELECT CASE WHEN get_byte(r, 0) < 205 THEN 'subtask' WHEN get_byte(r, 0) < 230 THEN 'create'
                          WHEN get_byte(r, 0) % 2 = 0 THEN 'report' ELSE 'account' END AS kind) AS chosen
$$;

CREATE FUNCTION pg_temp.realistic_holds(p_from int, p_to int) RETURNS void LANGUAGE sql VOLATILE AS $$
INSERT INTO moderation_targets (kind, id, subject)
SELECT DISTINCT b_kind, b_target, md5('realistic-author')::uuid FROM pg_temp.realistic_batch(p_from, p_to)
ON CONFLICT DO NOTHING;
INSERT INTO moderation_cases (id, target_kind, target_id, subject, source, private_evidence)
SELECT b_case, b_kind, b_target, md5('realistic-author')::uuid, 'own_review', '{}'
FROM pg_temp.realistic_batch(p_from, p_to);
INSERT INTO moderation_holds (case_id, target_kind, target_id, effect, starts_at, ends_at, active)
SELECT b_case, b_kind, b_target, b_effect, b_starts, b_ends, b_active
FROM pg_temp.realistic_batch(p_from, p_to);
$$;

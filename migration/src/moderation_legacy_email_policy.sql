-- New unpublished migration: default inbox, email only for requested access or important native changes.
CREATE OR REPLACE FUNCTION moderation_decide(p_actor uuid,p_request jsonb) RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE c moderation_cases; prior moderation_decisions; d uuid:=gen_random_uuid(); k uuid:=(p_request->>'request_key')::uuid;
    outcome text:=p_request->>'outcome'; effect text; deadline timestamptz; available timestamptz:=clock_timestamp();
    effect_before jsonb; measure_before jsonb;
    public jsonb; result jsonb; required text; historical boolean:=false; previous_sanctions integer; ladder_days integer; reviewed moderation_decisions; upheld moderation_holds;
BEGIN
    IF k IS NULL OR p_actor IS NULL THEN RAISE EXCEPTION 'Decision identity required'; END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('moderation-request:'||p_actor||':'||k,0));
    SELECT * INTO prior FROM moderation_decisions WHERE actor=p_actor AND request_key=k;
    IF FOUND THEN
      IF (prior.request-'reviewed_content')<>(p_request-'reviewed_content') THEN RAISE EXCEPTION 'Conflicting decision replay'; END IF;
      RETURN prior.public_statement;
    END IF;
    SELECT * INTO c FROM moderation_cases WHERE id=(p_request->>'case_id')::uuid;
    IF NOT FOUND THEN RAISE EXCEPTION 'Case not found'; END IF;
    PERFORM moderation_lock_target(c.target_kind,c.target_id);
    PERFORM pg_advisory_xact_lock(hashtextextended('moderation:'||c.target_kind||':'||c.target_id,0));
    SELECT * INTO c FROM moderation_cases WHERE id=c.id FOR UPDATE;
    IF c.revision IS DISTINCT FROM (p_request->>'expected_revision')::integer THEN RAISE EXCEPTION 'Stale case revision'; END IF;
    -- Native comparison under the target lock; never a caller-supplied importance flag.
    SELECT jsonb_build_object('enabled',state->'enabled','removed',state->'removed',
      'retired',state->'retired','withdrawn',state->'withdrawn') INTO effect_before
      FROM (SELECT moderation_effect(c.target_kind,c.target_id) AS state) current_effect;
    SELECT jsonb_build_object('effect',h.effect,'active',h.active,'rescinded',h.rescinded,'ends_at',h.ends_at)
      INTO measure_before FROM moderation_holds h WHERE h.case_id=c.id;
    IF c.target_kind='subtask' AND (p_request->>'reviewed_content_revision')::bigint IS DISTINCT FROM (SELECT content_revision FROM moderation_targets WHERE kind=c.target_kind AND id=c.target_id) THEN RAISE EXCEPTION 'Content changed or was not reviewed; reload the exact target'; END IF;
    IF p_request ? 'complaint_id' THEN
      SELECT d.* INTO reviewed FROM moderation_complaints a JOIN moderation_decisions d ON d.id=a.decision_id
       WHERE a.id=(p_request->>'complaint_id')::uuid AND a.case_id=c.id AND a.outcome_decision IS NULL;
      IF NOT FOUND THEN RAISE EXCEPTION 'Open complaint not found'; END IF;
    ELSE SELECT * INTO reviewed FROM moderation_decisions WHERE id=c.latest_decision; END IF;
    IF outcome='uphold' THEN
      SELECT * INTO upheld FROM moderation_holds WHERE case_id=c.id;
      IF NOT FOUND AND (reviewed.id IS NULL OR reviewed.outcome NOT IN ('warn','restore','uphold')) THEN RAISE EXCEPTION 'No existing measure to uphold; use an explicit decision'; END IF;
      IF reviewed.outcome IN ('warn','restore') OR (reviewed.outcome='uphold' AND reviewed.public_statement->>'upheld_measure' IS NULL) THEN upheld.effect:=NULL;upheld.ends_at:=NULL; END IF;
    END IF;
    IF outcome IS NULL OR outcome NOT IN ('provisional','uphold','remove','retire','restore','warn','restrict','authority_start','authority_change','authority_end')
      THEN RAISE EXCEPTION 'Explicit decision outcome required'; END IF;
    IF (c.target_kind<>'subtask' AND outcome IN ('provisional','remove','retire')) OR (c.target_kind='subtask' AND outcome='restrict') THEN RAISE EXCEPTION 'Outcome unsupported for this target kind'; END IF;
    IF outcome='warn' AND EXISTS(SELECT 1 FROM moderation_holds WHERE case_id=c.id AND active) THEN RAISE EXCEPTION 'Release the existing hold explicitly before a warning'; END IF;
    IF (c.source='authority_order')<>(outcome LIKE 'authority_%') THEN RAISE EXCEPTION 'Authority holds need their supported order lifecycle'; END IF;
    FOREACH required IN ARRAY ARRAY['rationale','ground','rule_version','automation','scope','redress'] LOOP
      IF length(trim(coalesce(p_request->>required,'')))<3 OR length(p_request->>required)>16000 THEN RAISE EXCEPTION 'Specific recipient-safe decision fields required: %',required; END IF;
    END LOOP;
    IF c.source='authority_order' THEN
      available:=greatest(available,c.notice_after);
    END IF;
    IF c.source='authority_order' AND length(coalesce(p_request->>'order_event_evidence',''))<3 THEN RAISE EXCEPTION 'Supported authority event required'; END IF;
    IF p_request ? 'notify_after' AND p_request->>'notify_after' IS NOT NULL THEN
      IF c.source<>'authority_order' AND (p_request->>'notify_after')::timestamptz>clock_timestamp() THEN RAISE EXCEPTION 'Only an authority instruction may defer this notice'; END IF;
      available:=greatest(available,(p_request->>'notify_after')::timestamptz);
      IF c.source='authority_order' THEN UPDATE moderation_cases SET notice_after=available WHERE id=c.id; END IF;
    END IF;
    IF p_request ? 'complaint_id' AND (p_request->'human_review' IS DISTINCT FROM 'true'::jsonb OR length(trim(coalesce(p_request->>'review_assessment','')))<3) THEN RAISE EXCEPTION 'Documented human complaint assessment required'; END IF;
    IF c.target_kind='account' AND (outcome='restrict' OR (outcome='uphold' AND upheld.effect IS NOT NULL)) THEN
      FOREACH required IN ARRAY ARRAY['misconduct_facts','proportionality','hearing'] LOOP
       IF length(trim(coalesce(p_request->>required,'')))<3 THEN RAISE EXCEPTION 'Specific account grounds, proportionality and hearing or urgency assessment required'; END IF;
      END LOOP;
    END IF;
    IF c.target_kind IN ('create','report') AND (outcome='restrict' OR (outcome='uphold' AND upheld.effect IS NOT NULL)) THEN
      FOREACH required IN ARRAY ARRAY['misconduct_facts','proportionality','hearing'] LOOP
       IF length(trim(coalesce(p_request->>required,'')))<3 THEN RAISE EXCEPTION 'Specific misconduct, proportionality and hearing assessment required'; END IF;
      END LOOP;
      IF p_request->>'duration_policy' IS NULL OR p_request->>'duration_policy' NOT IN ('published_ladder','individual_assessment') THEN RAISE EXCEPTION 'Explicit duration assessment required'; END IF;
      IF p_request->>'duration_policy'='published_ladder' AND EXISTS(SELECT 1 FROM moderation_holds h JOIN moderation_cases previous_case ON previous_case.id=h.case_id WHERE h.target_kind=c.target_kind AND h.target_id=c.target_id AND h.case_id<>c.id AND NOT h.rescinded AND NOT h.authority_order AND previous_case.source='legacy_import' AND NOT EXISTS(SELECT 1 FROM moderation_decisions d WHERE d.case_id=h.case_id AND d.actor<>'00000000-0000-0000-0000-000000000000'::uuid AND d.outcome IN ('restrict','uphold') AND d.request->'historical_basis_confirmed'='true'::jsonb)) THEN RAISE EXCEPTION 'Unassessed historic sanctions require individual duration assessment; do not infer their validity'; END IF;
      SELECT count(*) INTO previous_sanctions FROM moderation_holds WHERE target_kind=c.target_kind AND target_id=c.target_id AND case_id<>c.id AND NOT rescinded AND NOT authority_order AND starts_at<=clock_timestamp();
      ladder_days:=CASE previous_sanctions WHEN 0 THEN 3 WHEN 1 THEN 7 WHEN 2 THEN 30 END;
      IF p_request->>'duration_policy'='individual_assessment' AND length(trim(coalesce(p_request->>'duration_reason','')))<3 THEN RAISE EXCEPTION 'Explain the individually assessed duration'; END IF;
      IF c.target_kind='report' THEN
       FOREACH required IN ARRAY ARRAY['prior_warning','absolute_frequency','relative_frequency','seriousness','intent_assessment'] LOOP
        IF length(trim(coalesce(p_request->>required,'')))<1 THEN RAISE EXCEPTION 'Individual warning and misuse assessment required'; END IF;
       END LOOP;
      END IF;
    END IF;
    IF c.target_kind IN ('create','report','account') AND (outcome='restrict' OR (outcome='uphold' AND upheld.effect IS NOT NULL)) AND p_request->>'article23_applicability'='applies' THEN
      FOREACH required IN ARRAY ARRAY['article23_basis','misconduct_facts','proportionality','prior_warning','absolute_frequency','relative_frequency','seriousness','intent_assessment'] LOOP
       IF length(trim(coalesce(p_request->>required,'')))<1 THEN RAISE EXCEPTION 'Applicable Article 23 suspension requires substantiated applicability, prior warning and individual misuse assessment'; END IF;
      END LOOP;
    END IF;
    IF (p_request->>'scope') IS DISTINCT FROM (CASE c.target_kind WHEN 'subtask' THEN 'Diese Teilaufgabe auf Bootstrap Academy' WHEN 'create' THEN 'Erstellen von Teilaufgaben auf Bootstrap Academy' WHEN 'report' THEN 'Melden von Teilaufgaben auf Bootstrap Academy' ELSE 'Allgemeiner Kontozugang auf Bootstrap Academy; Rechtezugang bleibt erhalten' END) THEN RAISE EXCEPTION 'Unsupported scope; do not claim an unimplemented restriction'; END IF;
    deadline:=nullif(p_request->>'ends_at','')::timestamptz;
    IF c.target_kind IN ('create','report') AND (outcome='restrict' OR (outcome='uphold' AND upheld.effect IS NOT NULL)) THEN
      IF p_request->>'duration_policy'='published_ladder' THEN
        IF outcome='uphold' AND EXISTS(SELECT 1 FROM moderation_holds WHERE case_id=c.id) THEN SELECT ends_at INTO deadline FROM moderation_holds WHERE case_id=c.id;
        ELSE deadline:=clock_timestamp()+make_interval(days=>ladder_days); END IF;
      END IF;
      IF c.target_kind='report' AND deadline IS NULL AND (p_request->>'article23_applicability' IS DISTINCT FROM 'does_not_apply' OR length(trim(coalesce(p_request->>'article23_basis','')))<3) THEN RAISE EXCEPTION 'A finite report restriction is required unless non-application is actually established'; END IF;
    END IF;
    IF c.target_kind IN ('create','report','account') AND (outcome='restrict' OR (outcome='uphold' AND upheld.effect IS NOT NULL)) AND p_request->>'article23_applicability'='applies' AND deadline IS NULL THEN RAISE EXCEPTION 'Applicable Article 23 suspension requires an assessed finite period'; END IF;
    IF outcome='uphold' THEN
      deadline:=CASE WHEN reviewed.id IS DISTINCT FROM c.latest_decision THEN (reviewed.public_statement->>'ends_at')::timestamptz ELSE upheld.ends_at END;
    END IF;
    historical:=outcome='uphold' AND (reviewed.id IS DISTINCT FROM c.latest_decision OR EXISTS(SELECT 1 FROM moderation_targets WHERE kind=c.target_kind AND id=c.target_id AND withdrawn) OR upheld.ends_at<=clock_timestamp());
    historical:=coalesce(historical,false);
    IF NOT historical AND deadline IS NOT NULL AND deadline<=clock_timestamp() THEN RAISE EXCEPTION 'Restriction end must be in the future'; END IF;
    effect:=CASE outcome WHEN 'remove' THEN 'remove' WHEN 'retire' THEN 'retire' WHEN 'restrict' THEN 'restrict'
      WHEN 'authority_start' THEN CASE WHEN c.target_kind='subtask' THEN 'hide' ELSE 'restrict' END
      WHEN 'authority_change' THEN CASE WHEN c.target_kind='subtask' THEN 'hide' ELSE 'restrict' END
      WHEN 'provisional' THEN 'hide' WHEN 'uphold' THEN
       CASE WHEN reviewed.id IS NOT DISTINCT FROM c.latest_decision THEN upheld.effect
        ELSE CASE reviewed.outcome WHEN 'remove' THEN 'remove' WHEN 'retire' THEN 'retire' WHEN 'provisional' THEN 'hide' WHEN 'restrict' THEN 'restrict' ELSE reviewed.public_statement->>'upheld_measure' END END END;
    IF effect IS NOT NULL AND NOT historical AND outcome<>'uphold' THEN
      IF EXISTS(SELECT 1 FROM moderation_targets WHERE kind=c.target_kind AND id=c.target_id AND withdrawn) AND NOT (outcome='uphold' AND p_request ? 'complaint_id') THEN RAISE EXCEPTION 'Author withdrew target'; END IF;
      INSERT INTO moderation_holds(case_id,target_kind,target_id,effect,starts_at,ends_at,authority_order)
        VALUES(c.id,c.target_kind,c.target_id,effect,clock_timestamp(),deadline,c.source='authority_order')
        ON CONFLICT(case_id) DO UPDATE SET effect=excluded.effect,starts_at=excluded.starts_at,ends_at=excluded.ends_at,active=true,rescinded=false;
    ELSIF outcome IN ('restore','authority_end') THEN
      UPDATE moderation_holds SET active=false,rescinded=NOT (p_actor='00000000-0000-0000-0000-000000000000'::uuid AND p_request->'expired_measure'='true'::jsonb) WHERE case_id=c.id;
    END IF;
    public:=jsonb_build_object('decision_id',d,'case_id',c.id,'target_kind',c.target_kind,'target_id',c.target_id,
      'revision',c.revision+1,'reviewed_content_revision',p_request->'reviewed_content_revision','outcome',outcome,'decided_at',clock_timestamp(),'ends_at',deadline,'notice_available_at',available,
      'rationale',p_request->>'rationale','ground',p_request->>'ground','rule_version',p_request->>'rule_version',
      'automation',p_request->>'automation','scope',p_request->>'scope','redress',p_request->>'redress',
      'article23_applicability',coalesce(p_request->>'article23_applicability','undetermined'),'article23_basis',p_request->>'article23_basis','misconduct_facts',p_request->>'misconduct_facts','proportionality',p_request->>'proportionality','duration_policy',p_request->>'duration_policy','previous_unrescinded_sanctions',previous_sanctions,'hearing',p_request->>'hearing','human_review',p_request->'human_review','review_assessment',p_request->>'review_assessment','historical_only',historical,'reviewed_decision_id',CASE WHEN outcome='uphold' THEN reviewed.id END,'upheld_measure',CASE WHEN outcome='uphold' THEN effect END,
      'effect_before',effect_before,'measure_before',measure_before,
      'measure_after',(SELECT jsonb_build_object('effect',h.effect,'active',h.active,'rescinded',h.rescinded,'ends_at',h.ends_at) FROM moderation_holds h WHERE h.case_id=c.id),
      'effective',moderation_effect(c.target_kind,c.target_id)-'holds');
    INSERT INTO moderation_decisions(id,case_id,actor,request_key,request,outcome,public_statement) VALUES(d,c.id,p_actor,k,p_request,outcome,public);
    UPDATE moderation_cases SET revision=revision+1,latest_decision=d,
      closed_at=NULL,disposition_at=CASE WHEN outcome IN ('restore','authority_end','warn') OR historical OR (outcome='uphold' AND effect IS NULL) THEN clock_timestamp() END,
      work_review_at=CASE WHEN outcome='provisional' THEN clock_timestamp()+interval '7 days' END,notice_review_at=NULL WHERE id=c.id;
    IF historical THEN UPDATE moderation_holds SET active=false WHERE case_id=c.id AND (ends_at<=clock_timestamp() OR EXISTS(SELECT 1 FROM moderation_targets WHERE kind=c.target_kind AND id=c.target_id AND withdrawn)); END IF;
    -- Project after the immutable statement exists, in this same transaction.
    PERFORM moderation_project(c.target_kind,c.target_id);
    INSERT INTO moderation_messages(id,case_id,decision_id,recipient,audience,body,available_at,complaint_until)
      VALUES(gen_random_uuid(),c.id,d,c.subject,'author',public,available,NULL);
    IF c.notifier IS NOT NULL OR c.private_evidence ? 'notifier_contact' THEN
      INSERT INTO moderation_messages(id,case_id,decision_id,recipient,audience,body,available_at,complaint_until)
      VALUES(gen_random_uuid(),c.id,d,c.notifier,'notifier',jsonb_build_object('decision_id',d,'case_id',c.id,'outcome',outcome,
        'text',coalesce(nullif(p_request->>'notifier_rationale',''),p_request->>'rationale'),
        'automation',p_request->>'automation','redress',p_request->>'redress'),available,NULL);
    END IF;
    IF p_request ? 'complaint_id' THEN
      UPDATE moderation_complaints SET outcome_decision=d WHERE id=(p_request->>'complaint_id')::uuid AND case_id=c.id AND outcome_decision IS NULL;
      IF NOT FOUND THEN RAISE EXCEPTION 'Open complaint not found'; END IF;
    END IF;
    PERFORM moderation_update_review_due(c.id);
    RETURN public;
END $$;

-- Historical observations and their initial automatic expiry remain available
-- in the inbox. Only the email transport policy changes; statements stay intact.
-- Only changes with a concrete native before/after comparison are important.
-- Historical/unknown decisions and routine unchanged outcomes stay in the inbox.
CREATE FUNCTION moderation_important_email_decision(p_id uuid) RETURNS boolean LANGUAGE plpgsql STABLE AS $$
DECLARE d moderation_decisions; before_effect jsonb; after_effect jsonb; before_measure jsonb; after_measure jsonb; field text;
BEGIN
 SELECT * INTO d FROM moderation_decisions WHERE id=p_id;
 IF NOT FOUND OR d.outcome NOT IN ('provisional','remove','retire','restrict','restore','authority_start','authority_change','authority_end')
  OR d.public_statement->'historical_only' IS DISTINCT FROM 'false'::jsonb THEN RETURN false; END IF;
 before_effect:=d.public_statement->'effect_before';after_effect:=d.public_statement->'effective';
 before_measure:=d.public_statement->'measure_before';after_measure:=d.public_statement->'measure_after';
 -- Time can expire a different hold even while the target lock is held. A
 -- global difference alone is never evidence that this decision changed access.
 -- Rescinded-only history corrections likewise do not change the own measure.
 IF (before_measure->'active',before_measure->'effect',before_measure->'ends_at')
  IS NOT DISTINCT FROM (after_measure->'active',after_measure->'effect',after_measure->'ends_at') THEN RETURN false; END IF;
 FOREACH field IN ARRAY ARRAY['enabled','removed','retired','withdrawn'] LOOP
  IF jsonb_typeof(before_effect->field) IS DISTINCT FROM 'boolean' OR jsonb_typeof(after_effect->field) IS DISTINCT FROM 'boolean' THEN RETURN false; END IF;
 END LOOP;
 IF (before_effect->'enabled',before_effect->'removed',before_effect->'retired',before_effect->'withdrawn')
  IS DISTINCT FROM (after_effect->'enabled',after_effect->'removed',after_effect->'retired',after_effect->'withdrawn') THEN RETURN true; END IF;
 -- An actual change to an existing measure's effect or duration is important
 -- even when another independent measure currently masks the target state.
 IF before_measure->'active'='true'::jsonb AND after_measure->'active'='true'::jsonb
  AND (before_measure->'effect',before_measure->'ends_at') IS DISTINCT FROM (after_measure->'effect',after_measure->'ends_at') THEN RETURN true; END IF;
 -- Relevant release/expiry of a real measure; repeated restore is not a change.
 IF d.outcome IN ('restore','authority_end') AND before_measure->'active'='true'::jsonb
  AND after_measure->'active'='false'::jsonb AND before_measure->'rescinded'='false'::jsonb
  AND (before_measure->>'ends_at' IS NULL OR (before_measure->>'ends_at')::timestamptz>d.created_at
   OR (d.actor='00000000-0000-0000-0000-000000000000'::uuid AND d.request->'expired_measure'='true'::jsonb)) THEN RETURN true; END IF;
 RETURN false;
END $$;

CREATE FUNCTION moderation_message_email_policy(p_id uuid) RETURNS jsonb LANGUAGE sql STABLE AS $$
 SELECT jsonb_build_object('channel',CASE WHEN basis='important_change' THEN 'email' ELSE 'inbox_only' END,
  'basis',basis,'decision_id',decision_id)
 FROM (
  SELECT m.decision_id,CASE
   WHEN m.body->>'outcome'='legacy_observed' THEN 'legacy_observation'
   WHEN c.source='legacy_import' AND d.actor='00000000-0000-0000-0000-000000000000'::uuid
    AND d.outcome='restore' AND d.request @> '{"expired_measure":true,"expected_revision":1}'::jsonb
    AND EXISTS(SELECT 1 FROM moderation_decisions initial WHERE initial.case_id=c.id
     AND initial.outcome='legacy_observed' AND initial.actor='00000000-0000-0000-0000-000000000000'::uuid
     AND initial.created_at<=d.created_at)
    AND NOT EXISTS(SELECT 1 FROM moderation_decisions previous WHERE previous.case_id=c.id
     AND previous.id<>d.id AND previous.created_at<=d.created_at AND previous.outcome<>'legacy_observed') THEN 'legacy_expiry'
   WHEN m.audience='author' AND m.recipient=c.subject AND m.body=d.public_statement
    AND moderation_important_email_decision(d.id) THEN 'important_change'
   ELSE 'routine_or_unknown'
  END AS basis
  FROM moderation_messages m JOIN moderation_cases c ON c.id=m.case_id
  LEFT JOIN moderation_decisions d ON d.id=m.decision_id AND d.case_id=m.case_id
  WHERE m.id=p_id
 ) policy
$$;

-- A title is the existing parent challenge title, never an invented subtask title.
CREATE FUNCTION moderation_email_target_title(p_kind text,p_id uuid) RETURNS text LANGUAGE sql STABLE AS $$
 SELECT left(regexp_replace(c.title,'[[:cntrl:]]',' ','g'),200)
 FROM challenges_subtasks s JOIN challenges_challenges c ON c.task_id=s.task_id
 WHERE p_kind='subtask' AND s.id=p_id
$$;

CREATE FUNCTION moderation_message_email_context(p_id uuid) RETURNS jsonb LANGUAGE sql STABLE AS $$
 SELECT jsonb_strip_nulls(jsonb_build_object('target_kind',c.target_kind,
  'target_title',nullif(moderation_email_target_title(c.target_kind,c.target_id),''),
  'decision_automatic',CASE WHEN d.id IS NOT NULL THEN d.actor='00000000-0000-0000-0000-000000000000'::uuid END))
 FROM moderation_messages m JOIN moderation_cases c ON c.id=m.case_id
 LEFT JOIN moderation_decisions d ON d.id=m.decision_id AND d.case_id=m.case_id WHERE m.id=p_id
$$;

CREATE OR REPLACE FUNCTION moderation_claim(p_limit integer) RETURNS jsonb LANGUAGE sql AS $$
 WITH due AS (SELECT id FROM moderation_messages WHERE relayed_at IS NULL AND available_at<=clock_timestamp()
 AND next_attempt_at<=clock_timestamp() AND (lease_until IS NULL OR lease_until<=clock_timestamp())
 ORDER BY available_at FOR UPDATE SKIP LOCKED LIMIT least(greatest(p_limit,1),50)), claimed AS (
 UPDATE moderation_messages m SET generation=generation+1,attempts=attempts+1,lease_until=clock_timestamp()+interval '2 minutes'
 FROM due WHERE m.id=due.id RETURNING m.*)
 SELECT coalesce(jsonb_agg(jsonb_build_object('id',m.id,'case_id',m.case_id,'recipient',m.recipient,'audience',m.audience,'body',m.body,
 'email_policy',moderation_message_email_policy(m.id),
 'email_context',moderation_message_email_context(m.id),
 'available_at',m.available_at,'complaint_until',m.complaint_until,'generation',m.generation,
 'contact',CASE WHEN m.audience='notifier' THEN c.private_evidence->>'notifier_contact' ELSE c.private_evidence->>'author_contact' END)),'[]')
 FROM claimed m JOIN moderation_cases c ON c.id=m.case_id
$$;

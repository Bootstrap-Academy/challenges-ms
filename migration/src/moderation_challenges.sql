ALTER TABLE challenges_subtasks ADD COLUMN moderation_removed boolean NOT NULL DEFAULT false;
ALTER TABLE challenges_ban ADD COLUMN rescinded boolean NOT NULL DEFAULT false;

CREATE FUNCTION moderation_lock_target(p_kind text,p_id uuid) RETURNS void LANGUAGE plpgsql AS $$
BEGIN
 -- Content/version writes already share the advisory target lock. Account-row
 -- locking is an owning-backend concern; do not acquire remote identity state.
 RETURN;
END $$;

CREATE FUNCTION moderation_project(p_kind text,p_id uuid) RETURNS void LANGUAGE plpgsql AS $$
DECLARE state jsonb:=moderation_effect(p_kind,p_id); old_guard text:=current_setting('academy.moderation_write',true);
BEGIN
  PERFORM set_config('academy.moderation_write','authorized',true);
  IF p_kind='subtask' THEN
    UPDATE challenges_subtasks SET enabled=(state->>'enabled')::boolean,retired=(state->>'retired')::boolean,
      moderation_removed=(state->>'removed')::boolean WHERE id=p_id;
  ELSIF p_kind IN ('create','report') THEN
    INSERT INTO challenges_ban(id,user_id,start,"end",action,creator,reason,rescinded)
      SELECT c.id,c.subject,h.starts_at AT TIME ZONE 'UTC',h.ends_at AT TIME ZONE 'UTC',
        CASE p_kind WHEN 'create' THEN 'create' ELSE 'report' END::challenges_ban_action,
        coalesce(c.created_by,'00000000-0000-0000-0000-000000000000'::uuid),
        coalesce(d.public_statement->>'rationale','Übernommene Sperre: Die bisherige Begründung wird überprüft. Informationen und Beschwerde: /moderation oder hallo@bootstrap.academy.'),h.rescinded
      FROM moderation_cases c JOIN moderation_holds h ON h.case_id=c.id LEFT JOIN moderation_decisions d ON d.id=c.latest_decision
      WHERE c.target_kind=p_kind AND c.target_id=p_id
      ON CONFLICT(id) DO UPDATE SET start=excluded.start,"end"=excluded."end",reason=excluded.reason,rescinded=excluded.rescinded;
  ELSE RAISE EXCEPTION 'Unsupported Challenges moderation target'; END IF;
  PERFORM set_config('academy.moderation_write',coalesce(old_guard,''),true);
END $$;

CREATE FUNCTION moderation_adopt_target(p_kind text,p_id uuid,p_subject uuid) RETURNS void LANGUAGE plpgsql AS $$
DECLARE observed challenges_subtasks; effect text; legacy uuid;
BEGIN
 IF EXISTS(SELECT 1 FROM moderation_targets WHERE kind=p_kind AND id=p_id) THEN RETURN; END IF;
 IF p_kind='subtask' THEN
  SELECT * INTO observed FROM challenges_subtasks WHERE id=p_id;
  IF NOT FOUND OR observed.creator<>p_subject THEN RAISE EXCEPTION 'Target unavailable'; END IF;
  INSERT INTO moderation_targets(kind,id,subject) VALUES(p_kind,p_id,p_subject);
  FOR effect IN SELECT flag FROM (VALUES('hide',NOT observed.enabled),('retire',observed.retired),('remove',observed.moderation_removed)) flags(flag,present) WHERE present LOOP
   legacy:=gen_random_uuid();
   INSERT INTO moderation_cases(id,target_kind,target_id,subject,source,private_evidence,review_due_at)
    VALUES(legacy,p_kind,p_id,p_subject,'legacy_import',jsonb_build_object('observed_effect',effect,'observed_at',clock_timestamp(),'historical_ground','unknown','historical_notification','unknown'),clock_timestamp());
   INSERT INTO moderation_holds(case_id,target_kind,target_id,effect,starts_at) VALUES(legacy,p_kind,p_id,effect,clock_timestamp());
  END LOOP;
  PERFORM moderation_legacy_statements();
 ELSE
  INSERT INTO moderation_targets(kind,id,subject) VALUES(p_kind,p_id,p_subject);
 END IF;
END $$;

-- Legacy observations preserve actual current access, never infer a prior decision/delivery.
INSERT INTO moderation_targets(kind,id,subject,base_enabled,base_retired)
  SELECT 'subtask',id,creator,enabled,retired FROM challenges_subtasks;
INSERT INTO moderation_cases(id,target_kind,target_id,subject,source,notifier,private_evidence,review_due_at)
  SELECT r.id,'subtask',s.id,s.creator,'legacy_import',r.user_id,to_jsonb(r)||jsonb_build_object('imported_at',clock_timestamp(),'historical_notification','unknown'),clock_timestamp()
  FROM challenges_subtask_reports r JOIN challenges_subtasks s ON s.id=r.subtask_id;
INSERT INTO moderation_cases(id,target_kind,target_id,subject,source,private_evidence,review_due_at)
 SELECT gen_random_uuid(),'subtask',id,creator,'legacy_import',jsonb_build_object('observed_effect',effect,'historical_ground','unknown'),clock_timestamp()
 FROM challenges_subtasks CROSS JOIN (VALUES ('hide'),('retire')) flags(effect)
 WHERE (effect='hide' AND NOT enabled) OR (effect='retire' AND retired);
INSERT INTO moderation_holds(case_id,target_kind,target_id,effect,starts_at)
 SELECT id,target_kind,target_id,private_evidence->>'observed_effect',clock_timestamp()
 FROM moderation_cases WHERE private_evidence ? 'observed_effect';
UPDATE moderation_targets SET base_enabled=true,base_retired=false;

INSERT INTO moderation_targets(kind,id,subject)
 SELECT DISTINCT lower(action::text),user_id,user_id FROM challenges_ban ON CONFLICT DO NOTHING;
INSERT INTO moderation_cases(id,target_kind,target_id,subject,source,created_by,private_evidence,review_due_at)
 SELECT id,lower(action::text),user_id,user_id,'legacy_import',creator,to_jsonb(b)||jsonb_build_object('historical_notification','unknown'),clock_timestamp()
 FROM challenges_ban b;
INSERT INTO moderation_holds(case_id,target_kind,target_id,effect,starts_at,ends_at,active)
 SELECT id,lower(action::text),user_id,'restrict',start AT TIME ZONE 'UTC',"end" AT TIME ZONE 'UTC',true FROM challenges_ban;
UPDATE challenges_ban SET reason='Übernommene Sperre: Die bisherige Begründung wird überprüft. Informationen und Beschwerde: /moderation oder hallo@bootstrap.academy.';

CREATE FUNCTION moderation_subtask_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP='DELETE' THEN
    IF current_setting('academy.moderation_erasure_subject',true) IS DISTINCT FROM OLD.creator::text
      THEN RAISE EXCEPTION 'Use the reasoned moderation removal; author withdrawal needs its own authenticated path'; END IF;
    PERFORM moderation_withdraw_target('subtask',OLD.id);
    RETURN OLD;
  END IF;
  IF (NEW.enabled,NEW.retired,NEW.moderation_removed) IS DISTINCT FROM (OLD.enabled,OLD.retired,OLD.moderation_removed)
      AND current_setting('academy.moderation_write',true) IS DISTINCT FROM 'authorized'
    THEN RAISE EXCEPTION 'Use a reasoned moderation decision to change visibility'; END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER moderation_subtask_guard BEFORE UPDATE OR DELETE ON challenges_subtasks FOR EACH ROW EXECUTE FUNCTION moderation_subtask_guard();
CREATE FUNCTION moderation_ban_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF current_setting('academy.moderation_write',true)='authorized' THEN RETURN coalesce(NEW,OLD); END IF;
  IF TG_OP='DELETE' AND current_setting('academy.moderation_erasure_subject',true)=OLD.user_id::text THEN RETURN OLD; END IF;
  IF TG_OP='UPDATE' AND NEW.creator IS DISTINCT FROM OLD.creator AND (to_jsonb(NEW)-'creator')=(to_jsonb(OLD)-'creator') THEN RETURN NEW; END IF;
  RAISE EXCEPTION 'Use a reasoned moderation decision to change a sanction';
END $$;
CREATE TRIGGER moderation_ban_guard BEFORE INSERT OR UPDATE OR DELETE ON challenges_ban FOR EACH ROW EXECUTE FUNCTION moderation_ban_guard();

-- All administrative content edits share the same target lock used by cases.
-- A new content version invalidates a stale review, without rewriting evidence.
CREATE FUNCTION moderation_content_changed() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE target uuid; owner uuid;
BEGIN
 IF TG_TABLE_NAME='challenges_subtasks' THEN target:=NEW.id; ELSE target:=NEW.subtask_id; END IF;
 IF TG_OP='UPDATE' AND to_jsonb(NEW)=to_jsonb(OLD) THEN RETURN NEW; END IF;
 IF TG_TABLE_NAME='challenges_subtasks' AND (to_jsonb(NEW)-ARRAY['enabled','retired','moderation_removed'])=(to_jsonb(OLD)-ARRAY['enabled','retired','moderation_removed']) THEN RETURN NEW; END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('moderation:subtask:'||target,0));
 SELECT creator INTO owner FROM challenges_subtasks WHERE id=target;
 PERFORM moderation_adopt_target('subtask',target,owner);
 UPDATE moderation_targets SET content_revision=content_revision+1 WHERE kind='subtask' AND id=target;
 UPDATE moderation_cases SET closed_at=NULL,review_due_at=clock_timestamp() WHERE target_kind='subtask' AND target_id=target AND EXISTS(SELECT 1 FROM moderation_holds WHERE case_id=moderation_cases.id AND active);
 RETURN NEW;
END $$;
CREATE TRIGGER moderation_content_changed BEFORE UPDATE ON challenges_subtasks FOR EACH ROW EXECUTE FUNCTION moderation_content_changed();
DO $$ DECLARE relation_name text; BEGIN
 FOR relation_name IN SELECT table_name FROM information_schema.columns WHERE table_schema='public' AND column_name='subtask_id' AND table_name IN ('challenges_questions','challenges_multiple_choice_quizes','challenges_matchings','challenges_coding_challenges') LOOP
  EXECUTE format('CREATE TRIGGER moderation_content_changed BEFORE UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION moderation_content_changed()',relation_name);
 END LOOP;
END $$;

SELECT moderation_legacy_statements();
CREATE FUNCTION moderation_disposal_adapter(p_case uuid) RETURNS void LANGUAGE plpgsql AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM challenges_ban WHERE id=p_case AND NOT rescinded AND ("end" IS NULL OR "end">clock_timestamp() AT TIME ZONE 'UTC')) THEN RAISE EXCEPTION 'An active sanction prevents disposal'; END IF;
 PERFORM set_config('academy.moderation_write','authorized',true);
 DELETE FROM challenges_ban WHERE id=p_case;
 PERFORM set_config('academy.moderation_write','',true);
 DELETE FROM challenges_subtask_reports WHERE id=p_case;
END $$;

CREATE TABLE moderation_private_minimizations (id uuid PRIMARY KEY DEFAULT gen_random_uuid(),case_id uuid NOT NULL,field text NOT NULL,created_at timestamptz NOT NULL DEFAULT clock_timestamp(),relayed_at timestamptz);
CREATE FUNCTION moderation_minimize_adapter(p_case uuid,p_field text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN
 IF p_field='comment' THEN UPDATE challenges_subtask_reports SET comment='' WHERE id=p_case; END IF;
 IF p_field IN ('author_contact','notifier_contact') THEN INSERT INTO moderation_private_minimizations(case_id,field) VALUES(p_case,p_field); END IF;
END $$;

CREATE FUNCTION moderation_pending_work(p_case uuid) RETURNS boolean LANGUAGE sql STABLE AS $$ SELECT false $$;

-- Minimal notifier receipt survives target/account removal with its case. It
-- binds the exact request digest; the original private text stays in one case.
CREATE TABLE moderation_report_receipts (
 id uuid PRIMARY KEY REFERENCES moderation_cases(id) ON DELETE CASCADE,
 actor uuid NOT NULL, request_hash text NOT NULL, receipt jsonb NOT NULL
);
CREATE TRIGGER moderation_report_receipts_immutable BEFORE UPDATE OR DELETE ON moderation_report_receipts FOR EACH ROW EXECUTE FUNCTION moderation_immutable();

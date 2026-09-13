CREATE OR REPLACE FUNCTION moderation_content_changed() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE target uuid; owner uuid;
BEGIN
 IF TG_TABLE_NAME='challenges_subtasks' THEN target:=NEW.id; ELSE target:=NEW.subtask_id; END IF;
 IF TG_OP='UPDATE' AND to_jsonb(NEW)=to_jsonb(OLD) THEN PERFORM moderation_update_review_due(id) FROM moderation_cases WHERE target_kind='subtask' AND target_id=target;
 RETURN NEW; END IF;
 IF TG_TABLE_NAME='challenges_subtasks' AND (to_jsonb(NEW)-ARRAY['enabled','retired','moderation_removed'])=(to_jsonb(OLD)-ARRAY['enabled','retired','moderation_removed']) THEN PERFORM moderation_update_review_due(id) FROM moderation_cases WHERE target_kind='subtask' AND target_id=target;
 RETURN NEW; END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('moderation:subtask:'||target,0));
 SELECT creator INTO owner FROM challenges_subtasks WHERE id=target;
 PERFORM moderation_adopt_target('subtask',target,owner);
 UPDATE moderation_targets SET content_revision=content_revision+1 WHERE kind='subtask' AND id=target;
 UPDATE moderation_cases SET closed_at=NULL,work_review_at=clock_timestamp() WHERE target_kind='subtask' AND target_id=target AND EXISTS(SELECT 1 FROM moderation_holds WHERE case_id=moderation_cases.id AND active);
 PERFORM moderation_update_review_due(id) FROM moderation_cases WHERE target_kind='subtask' AND target_id=target;
 RETURN NEW;
END $$;

-- Synthetic owning-service acceptance. Run after migrations, always rolled back.
BEGIN;
DO $$
DECLARE author uuid:=gen_random_uuid(); reporter uuid:=gen_random_uuid(); moderator uuid:=gen_random_uuid(); second_moderator uuid:=gen_random_uuid();
 target uuid:=gen_random_uuid(); parent uuid:=gen_random_uuid(); c1 uuid:=gen_random_uuid(); c2 uuid:=gen_random_uuid();
 command jsonb; first jsonb; second jsonb; result jsonb; complaint uuid:=gen_random_uuid(); complaint2 uuid:=gen_random_uuid(); c3 uuid:=gen_random_uuid(); n integer;
BEGIN
 INSERT INTO challenges_tasks VALUES(parent,author,now());
 INSERT INTO challenges_subtasks(id,task_id,creator,creation_timestamp,xp,coins,ty) VALUES(target,parent,author,now(),0,0,'question');
 PERFORM moderation_open(c1,reporter,'subtask',target,author,'user_report',reporter,'{"comment":"PRIVATE REPORTER SENTINEL"}');
 command:=jsonb_build_object('request_key',gen_random_uuid(),'case_id',c1,'expected_revision',0,'reviewed_content_revision',0,'outcome','provisional','rationale','A structured factual allegation awaits human examination. No violation has been established.','ground','AGB 14.3 quality review','rule_version','Synthetic rules version','automation','Automatic provisional action; no human finding','scope','Diese Teilaufgabe auf Bootstrap Academy','redress','Six months human review and judicial remedies');
 first:=moderation_decide(moderator,command);
 ASSERT NOT (SELECT enabled FROM challenges_subtasks WHERE id=target),'restriction must project atomically';
 ASSERT first=moderation_decide(moderator,command),'idempotent response';
 ASSERT (SELECT count(*) FROM moderation_decisions WHERE case_id=c1)=1,'replay appended a duplicate';
 ASSERT moderation_inbox(author)::text NOT LIKE '%PRIVATE REPORTER%','private text leaked';
 ASSERT moderation_inbox(author)::text NOT LIKE '%'||reporter::text||'%','reporter identity leaked';
 ASSERT jsonb_array_length(moderation_inbox(reporter))=2,'acknowledgement and outcome required';
 BEGIN
   PERFORM moderation_decide(moderator,command||'{"rationale":"conflicting"}');
   RAISE EXCEPTION 'conflicting replay accepted';
 EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM='Conflicting decision replay'; END;
 BEGIN
   UPDATE challenges_subtasks SET enabled=true WHERE id=target;
   RAISE EXCEPTION 'direct visibility mutation accepted';
 EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM LIKE 'Use a reasoned moderation decision%'; END;
 BEGIN
   DELETE FROM challenges_subtasks WHERE id=target;
   RAISE EXCEPTION 'direct removal accepted';
 EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM LIKE 'Use the reasoned moderation removal%'; END;
 BEGIN
   PERFORM moderation_decide(moderator,command||jsonb_build_object('request_key',gen_random_uuid(),'expected_revision',1,'outcome','warn'));
   RAISE EXCEPTION 'warning stranded active hold';
 EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM='Release the existing hold explicitly before a warning'; END;
 PERFORM moderation_open(c3,moderator,'create',author,author,'own_review',NULL,'{}');
 BEGIN
   PERFORM moderation_decide(moderator,command||jsonb_build_object('request_key',gen_random_uuid(),'case_id',c3,'outcome','retire','scope','Erstellen von Teilaufgaben auf Bootstrap Academy'));
   RAISE EXCEPTION 'unsupported ban retirement accepted';
 EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM='Outcome unsupported for this target kind'; END;
 ASSERT NOT EXISTS(SELECT 1 FROM challenges_ban WHERE id=c3);
 ASSERT (moderation_effect('create',author)->>'enabled')::boolean;
 -- A second independent authority hold survives ordinary restoration.
 PERFORM moderation_open(c2,moderator,'subtask',target,author,'authority_order',NULL,'{"authority":"Synthetic authority","order_reference":"Test order","notification_instructions":"Notify now"}');
 second:=moderation_decide(moderator,command||jsonb_build_object('request_key',gen_random_uuid(),'case_id',c2,'outcome','authority_start','order_event_evidence','Validated synthetic order'));
 result:=moderation_decide(moderator,command||jsonb_build_object('request_key',gen_random_uuid(),'expected_revision',1,'outcome','restore'));
 ASSERT NOT (SELECT enabled FROM challenges_subtasks WHERE id=target),'ordinary restoration defeated authority hold';
 ASSERT NOT (result->'effective'->>'enabled')::boolean,'response invented restoration';
 BEGIN
   PERFORM moderation_decide(moderator,command||jsonb_build_object('request_key',gen_random_uuid(),'case_id',c2,'expected_revision',1,'outcome','restore'));
   RAISE EXCEPTION 'ordinary command revoked order';
 EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM='Authority holds need their supported order lifecycle'; END;
 -- A complaint is target/recipient bound, durable and replayable.
 PERFORM moderation_complain(author,jsonb_build_object('id',complaint,'decision_id',first->'decision_id','text','Please consider the correction.'));
 PERFORM moderation_complain(author,jsonb_build_object('id',complaint,'decision_id',first->'decision_id','text','Please consider the correction.'));
 ASSERT (SELECT count(*) FROM moderation_complaints WHERE id=complaint)=1;
 BEGIN
   PERFORM moderation_complain(author,jsonb_build_object('id',complaint));
   RAISE EXCEPTION 'incomplete complaint replay accepted';
 EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM='Complete complaint identity and text required'; END;
 PERFORM moderation_complain(reporter,jsonb_build_object('id',complaint2,'decision_id',first->'decision_id','text','Please review the notifier outcome.'));
 BEGIN
   PERFORM moderation_complain(gen_random_uuid(),jsonb_build_object('id',gen_random_uuid(),'decision_id',first->'decision_id','text','Wrong recipient.'));
   RAISE EXCEPTION 'wrong recipient complaint accepted';
 EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM='Decision unavailable for this recipient'; END;
 BEGIN
   PERFORM moderation_decide(moderator,command||jsonb_build_object('request_key',gen_random_uuid(),'expected_revision',2,'outcome','restore','complaint_id',complaint));
   RAISE EXCEPTION 'unrecorded human review accepted';
 EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM='Documented human complaint assessment required'; END;
 result:=moderation_decide(second_moderator,command||jsonb_build_object('request_key',gen_random_uuid(),'expected_revision',2,'outcome','restore','complaint_id',complaint,'human_review',true,'review_assessment','Human test review considered the actual complaint and correction.'));
 ASSERT (SELECT outcome_decision IS NOT NULL FROM moderation_complaints WHERE id=complaint);
 ASSERT (SELECT closed_at IS NULL AND review_due_at IS NOT NULL FROM moderation_cases WHERE id=c1),'another unresolved complaint disappeared from active work';
 ASSERT (SELECT complaint_until IS NULL AND informed_at IS NULL FROM moderation_messages WHERE decision_id=(first->>'decision_id')::uuid AND recipient=author),'queue creation invented actual notification';
 PERFORM moderation_opened(author,(SELECT id FROM moderation_messages WHERE decision_id=(first->>'decision_id')::uuid AND recipient=author));
 ASSERT (SELECT informed_at IS NOT NULL AND complaint_until>informed_at+interval '6 months' FROM moderation_messages WHERE decision_id=(first->>'decision_id')::uuid AND recipient=author);
 ASSERT moderation_review_minimum('2024-08-31 22:00+02')='2025-03-01 00:00+01'::timestamptz,'calendar month-end clamp';
 ASSERT moderation_review_minimum('2024-08-29 22:00+02')='2025-03-01 00:00+01'::timestamptz,'corresponding final date inclusive';
 -- Withdrawal is final for public republication; case and safe statement survive.
 PERFORM set_config('academy.moderation_erasure_subject',author::text,true);
 DELETE FROM challenges_subtasks WHERE id=target;
 result:=moderation_decide(moderator,command||jsonb_build_object('request_key',gen_random_uuid(),'case_id',c2,'expected_revision',1,'outcome','authority_end','order_event_evidence','Synthetic expiry or revocation evidence'));
 ASSERT NOT (result->'effective'->>'enabled')::boolean;
 ASSERT (result->'effective'->>'withdrawn')::boolean;
 ASSERT NOT EXISTS(SELECT 1 FROM challenges_subtasks WHERE id=target);
 result:=moderation_decide(second_moderator,command||jsonb_build_object('request_key',gen_random_uuid(),'expected_revision',3,'outcome','uphold','complaint_id',complaint2,'human_review',true,'review_assessment','Human historical assessment; withdrawn content stays unavailable.'));
 ASSERT (result->>'historical_only')::boolean AND NOT (result->'effective'->>'enabled')::boolean,'historical adverse disposition must not republish';
 ASSERT (SELECT outcome_decision IS NOT NULL FROM moderation_complaints WHERE id=complaint2);
 BEGIN
   PERFORM moderation_escalate(moderator,jsonb_build_object('id',gen_random_uuid(),'case_id',c1,'facts','Synthetic facts','assessment','Synthetic assessment','human_responsibility','Synthetic reviewer'));
   RAISE EXCEPTION 'missing escalation kind accepted';
 EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM='Explicit escalation record required'; END;
 BEGIN
   PERFORM moderation_escalate(moderator,jsonb_build_object('id',gen_random_uuid(),'case_id',c1,'kind','article18_transmission','facts','Synthetic facts','assessment','Synthetic assessment','human_responsibility','Synthetic reviewer','authority','Synthetic authority','transmission_evidence','Synthetic evidence'));
   RAISE EXCEPTION 'transmission without actual timestamp accepted';
 EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM='Actual transmission evidence required'; END;

 ASSERT jsonb_array_length(moderation_inbox(author))>=4;
 -- Decisions cannot be rewritten or erased by an ordinary mutation.
 BEGIN
   DELETE FROM moderation_decisions WHERE id=(first->>'decision_id')::uuid;
   RAISE EXCEPTION 'immutable evidence deleted';
 EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM LIKE 'Moderation evidence is immutable%'; END;
 -- Failed statement validation rolls back the full decision, including holds.
 n:=(SELECT count(*) FROM moderation_decisions);
 BEGIN
   PERFORM moderation_decide(second_moderator,command||jsonb_build_object('request_key',gen_random_uuid(),'expected_revision',4,'rationale',''));
   RAISE EXCEPTION 'missing reason accepted';
 EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM LIKE 'Specific recipient-safe decision fields required%'; END;
 ASSERT (SELECT count(*) FROM moderation_decisions)=n;
 RAISE NOTICE 'PASS: atomic state/statement, privacy, replay, independent holds, authority, recipient complaints, human review, withdrawal and immutable evidence';
END $$;
ROLLBACK;

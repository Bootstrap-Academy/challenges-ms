-- Synthetic lifecycle sequences; no time travel outside the rolled-back fixture.
BEGIN;
DO $$
DECLARE person uuid:=gen_random_uuid(); mod uuid:=gen_random_uuid(); c uuid:=gen_random_uuid(); ordinary uuid:=gen_random_uuid();
 cmd jsonb; first jsonb; complaint uuid:=gen_random_uuid(); hold_review uuid:=gen_random_uuid(); disposal jsonb; deferred timestamptz:=clock_timestamp()+interval '2 days';
BEGIN
 PERFORM moderation_open(c,mod,'create',person,person,'authority_order',NULL,'{"authority":"Synthetic authority","order_reference":"Only a synthetic order","notification_instructions":"Synthetic valid deferral"}');
 cmd:=jsonb_build_object('request_key',gen_random_uuid(),'case_id',c,'expected_revision',0,'outcome','authority_start','rationale','Synthetic supported order','ground','Synthetic order ground','rule_version','Synthetic order','automation','Human synthetic decision','scope','Erstellen von Teilaufgaben auf Bootstrap Academy','redress','Six calendar months and judicial remedies','ends_at',clock_timestamp()+interval '1 hour','notify_after',deferred,'order_event_evidence','Synthetic exact instruction');
 first:=moderation_decide(mod,cmd);
 UPDATE moderation_holds SET ends_at=clock_timestamp()-interval '1 second' WHERE case_id=c;
 PERFORM moderation_maintenance();
 ASSERT NOT EXISTS(SELECT 1 FROM moderation_holds WHERE case_id=c AND active),'expiry must release its own hold';
 ASSERT NOT EXISTS(SELECT 1 FROM moderation_messages WHERE case_id=c AND available_at<deferred),'expiry disclosed deferred order';
 ASSERT NOT EXISTS(SELECT 1 FROM jsonb_array_elements(moderation_inbox(person)) m WHERE m->>'case_id'=c::text),'deferred case leaked into inbox';
 PERFORM moderation_escalate(mod,jsonb_build_object('id',gen_random_uuid(),'case_id',c,'kind','notice_instruction','facts','Synthetic release instruction','assessment','Notice restriction is now lifted by actual instruction','human_responsibility','Synthetic test operator','notice_after',clock_timestamp(),'instruction_evidence','Synthetic verified release'));
 ASSERT EXISTS(SELECT 1 FROM jsonb_array_elements(moderation_inbox(person)) m WHERE m->>'case_id'=c::text),'valid later release did not expose truthful original history';
 ASSERT (SELECT count(*) FROM moderation_decisions WHERE case_id=c)=2;
 -- Expiry is not rescission; human historical review does not reimpose it.
 PERFORM moderation_open(ordinary,mod,'create',person,person,'own_review',NULL,'{"comment":"private evidence for test","attachments":"separate unnecessary attachment"}');
 cmd:=cmd-'notify_after'-'order_event_evidence'||jsonb_build_object('request_key',gen_random_uuid(),'case_id',ordinary,'expected_revision',0,'outcome','restrict','misconduct_facts','Synthetic independently reviewed misconduct','proportionality','Synthetic mildest appropriate response','hearing','Synthetic hearing considered','duration_policy','published_ladder');
 first:=moderation_decide(mod,cmd);
 ASSERT (first->>'ends_at')::timestamptz BETWEEN clock_timestamp()+interval '2 days 23 hours' AND clock_timestamp()+interval '3 days 1 minute';
 UPDATE moderation_holds SET ends_at=clock_timestamp()-interval '1 second' WHERE case_id=ordinary;
 PERFORM moderation_maintenance();
 ASSERT (SELECT NOT active AND NOT rescinded FROM moderation_holds WHERE case_id=ordinary),'normal expiry erased sanction history';
 PERFORM moderation_complain(person,jsonb_build_object('id',complaint,'decision_id',first->'decision_id','text','Historical human complaint after expiry'));
 first:=moderation_decide(mod,cmd||jsonb_build_object('request_key',gen_random_uuid(),'expected_revision',2,'outcome','uphold','complaint_id',complaint,'human_review',true,'review_assessment','Synthetic review of ended historical sanction'));
 ASSERT (first->>'historical_only')::boolean AND NOT EXISTS(SELECT 1 FROM moderation_holds WHERE case_id=ordinary AND active),'historical complaint reimposed sanction';
 ASSERT (SELECT disposition_at IS NOT NULL FROM moderation_cases WHERE id=ordinary),'historical disposition lost closure path';
 -- Applicable creator misuse follows the same warning/finite assessment guard.
 DECLARE c23 uuid:=gen_random_uuid(); person23 uuid:=gen_random_uuid(); assessment jsonb;
 BEGIN
  PERFORM moderation_open(c23,mod,'create',person23,person23,'own_review',NULL,'{}');
  assessment:=cmd-'complaint_id'-'human_review'-'review_assessment'||jsonb_build_object('case_id',c23,'request_key',gen_random_uuid(),'expected_revision',0,'outcome','restrict','duration_policy','individual_assessment','duration_reason','Synthetic individual duration','ends_at',NULL,'article23_applicability','applies','article23_basis','Synthetic actual applicability');
  BEGIN PERFORM moderation_decide(mod,assessment); RAISE EXCEPTION 'Accepted missing warning'; EXCEPTION WHEN raise_exception THEN IF SQLERRM='Accepted missing warning' THEN RAISE; END IF; END;
  assessment:=assessment||'{"prior_warning":"Synthetic actual warning","absolute_frequency":"12","relative_frequency":"12 of 14","seriousness":"Synthetic seriousness","intent_assessment":"Synthetic intention"}'::jsonb;
  BEGIN PERFORM moderation_decide(mod,assessment); RAISE EXCEPTION 'Accepted permanent applicable suspension'; EXCEPTION WHEN raise_exception THEN IF SQLERRM='Accepted permanent applicable suspension' THEN RAISE; END IF; END;
  PERFORM moderation_decide(mod,assessment||jsonb_build_object('ends_at',clock_timestamp()+interval '3 days'));
  ASSERT EXISTS(SELECT 1 FROM moderation_holds WHERE case_id=c23 AND active AND ends_at IS NOT NULL);
  PERFORM moderation_withdraw_target('create',person23);
  ASSERT NOT EXISTS(SELECT 1 FROM moderation_holds WHERE case_id=c23 AND active) AND EXISTS(SELECT 1 FROM moderation_cases WHERE id=c23 AND disposition_at IS NOT NULL),'withdrawal stranded indefinite lifecycle';
 END;
 -- A field-scoped exception is durable until explicitly released.
 PERFORM moderation_retention(mod,jsonb_build_object('id',hold_review,'case_id',ordinary,'action','retain','reason','Synthetic pending concrete claim','legal_or_claim_basis','Synthetic claim document','necessary_fields',jsonb_build_array('comment'),'review_at',clock_timestamp()+interval '1 day'));
 BEGIN
  PERFORM moderation_retention(mod,jsonb_build_object('id',gen_random_uuid(),'case_id',ordinary,'action','minimize','reason','Synthetic cleanup','unnecessary_fields',jsonb_build_array('comment')));
  RAISE EXCEPTION 'held evidence deleted';
 EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM='Release or amend the applicable field retention before minimization'; END;
 PERFORM moderation_retention(mod,jsonb_build_object('id',gen_random_uuid(),'case_id',ordinary,'action','minimize','reason','Attachment specifically unnecessary for this claim','unnecessary_fields',jsonb_build_array('attachments')));
 ASSERT (SELECT private_evidence ? 'comment' AND NOT private_evidence ? 'attachments' FROM moderation_cases WHERE id=ordinary);
 -- Conservative actual-notice closure: never creation/SMTP attempt time.
 PERFORM moderation_maintenance(); ASSERT (SELECT closed_at IS NULL FROM moderation_cases WHERE id=ordinary);
 UPDATE moderation_messages SET informed_at=clock_timestamp()-interval '2 years',complaint_until=clock_timestamp()-interval '1 year' WHERE case_id=ordinary;
 PERFORM moderation_maintenance(); ASSERT (SELECT closed_at IS NOT NULL FROM moderation_cases WHERE id=ordinary);
 UPDATE moderation_cases SET closed_at=clock_timestamp()-interval '13 months' WHERE id=ordinary;
 disposal:=jsonb_build_object('id',gen_random_uuid(),'case_id',ordinary,'action','dispose','reason','Synthetic reviewed closure and expiry','claims_and_retention_checked',true);
 BEGIN PERFORM moderation_retention(mod,disposal); RAISE EXCEPTION 'active claim exception ignored'; EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM='Review and explicitly release the documented retention exception'; END;
 PERFORM moderation_retention(mod,jsonb_build_object('id',gen_random_uuid(),'case_id',ordinary,'action','release_retention','retention_id',hold_review,'reason','Synthetic claim actually ended'));
 ASSERT moderation_retention(mod,disposal); ASSERT moderation_retention(mod,disposal),'lost disposal response not replayable';
 ASSERT NOT EXISTS(SELECT 1 FROM moderation_cases WHERE id=ordinary);
 ASSERT NOT EXISTS(SELECT 1 FROM moderation_retention_reviews WHERE case_id=ordinary),'personal disposal review bodies retained';
 BEGIN PERFORM moderation_retention(mod,disposal||'{"reason":"Changed replay"}'); RAISE EXCEPTION 'changed disposal replay accepted'; EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM='Conflicting disposal replay'; END;
 BEGIN PERFORM moderation_open(ordinary,mod,'create',person,person,'own_review',NULL,'{}'); RAISE EXCEPTION 'disposed identity reused'; EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM='Disposed case identity cannot be reused'; END;
 -- Unknown legacy observation cannot establish escalation of a new sanction.
 c:=gen_random_uuid(); ordinary:=gen_random_uuid();
 PERFORM moderation_open(c,mod,'create',person,person,'legacy_import',NULL,'{"historical_ground":"unknown"}');
 INSERT INTO moderation_holds(case_id,target_kind,target_id,effect,starts_at,ends_at,active) VALUES(c,'create',person,'restrict',clock_timestamp()-interval '1 year',clock_timestamp()-interval '11 months',false);
 PERFORM moderation_open(ordinary,mod,'create',person,person,'own_review',NULL,'{}');
 BEGIN PERFORM moderation_decide(mod,cmd||jsonb_build_object('request_key',gen_random_uuid(),'case_id',ordinary)); RAISE EXCEPTION 'unknown historical basis escalated sanction'; EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM='Unassessed historic sanctions require individual duration assessment; do not infer their validity'; END;
 RAISE NOTICE 'PASS lifecycle: deferred authority expiry/release, expiry vs rescission, historical human disposition, actual-clock closure, field-scoped retention, terminal replay and no escalation from unassessed history';
END $$;
ROLLBACK;

-- Reversible synthetic controls for independent work and exact typed complaints.
BEGIN;
DO $$
DECLARE person uuid:=gen_random_uuid(); actor uuid:=gen_random_uuid(); c uuid:=gen_random_uuid();
 first jsonb; cmd jsonb; complaint jsonb; urgent uuid:=gen_random_uuid(); due timestamptz; changed jsonb;
BEGIN
 PERFORM moderation_open(c,actor,'create',person,person,'own_review',NULL,'{"author_contact":"unreachable@example.invalid","comment":"necessary evidence"}');
 cmd:=jsonb_build_object('case_id',c,'request_key',gen_random_uuid(),'expected_revision',0,'outcome','warn','rationale','Individually reviewed synthetic warning','ground','Synthetic specific ground','rule_version','Synthetic original reference','automation','Human fixture decision','scope','Erstellen von Teilaufgaben auf Bootstrap Academy','redress','Human review and independent remedies');
 first:=moderation_decide(actor,cmd);
 ASSERT (SELECT closed_at IS NULL AND review_due_at IS NOT NULL FROM moderation_cases WHERE id=c),'unknown notice must remain discoverable';
 complaint:=jsonb_build_object('id',gen_random_uuid(),'decision_id',first->'decision_id','text','123');
 PERFORM moderation_complain(person,complaint);PERFORM moderation_complain(person,complaint);
 FOR changed IN SELECT value FROM jsonb_array_elements(jsonb_build_array(complaint||'{"text":123}',complaint||'{"text":null}',complaint-'text',complaint||jsonb_build_object('id',gen_random_uuid(),'text','{"claim":"object"}'::jsonb))) LOOP
  BEGIN PERFORM moderation_complain(person,changed);RAISE EXCEPTION 'invalid typed complaint accepted';EXCEPTION WHEN raise_exception THEN ASSERT SQLERRM<>'invalid typed complaint accepted';END;
 END LOOP;
 PERFORM moderation_escalate(actor,jsonb_build_object('id',urgent,'case_id',c,'kind','article18_assessment','facts','Specific synthetic urgent facts','assessment','Unresolved urgent human assessment','human_responsibility','Synthetic responsible reviewer'));
 SELECT review_due_at INTO due FROM moderation_cases WHERE id=c;
 PERFORM moderation_complain(person,complaint||jsonb_build_object('id',gen_random_uuid(),'text','Independent ordinary complaint'));
 ASSERT (SELECT review_due_at=due FROM moderation_cases WHERE id=c),'ordinary complaint postponed independent urgent work';
 PERFORM moderation_retention(actor,jsonb_build_object('id',gen_random_uuid(),'case_id',c,'action','minimize','reason','Specific unnecessary failed contact','unnecessary_fields',jsonb_build_array('author_contact'),'open_complaints_considered',true,'contact_necessity_assessment','Address no longer needed for the reviewed case','remaining_remedy_access','Retained inbox and human contact remain accessible','unnotified_rights_preserved',true,'review_at',clock_timestamp()+interval '1 day'));
 ASSERT (SELECT NOT private_evidence ? 'author_contact' AND private_evidence->>'comment'='necessary evidence' AND closed_at IS NULL AND review_due_at<=(SELECT recorded_at FROM moderation_escalations WHERE id=urgent) FROM moderation_cases WHERE id=c);
 ASSERT NOT EXISTS(SELECT 1 FROM moderation_messages WHERE case_id=c AND (informed_at IS NOT NULL OR complaint_until IS NOT NULL)),'necessity review invented notice or appeal expiry';
 changed:=moderation_decide(actor,cmd||jsonb_build_object('request_key',gen_random_uuid(),'expected_revision',1,'outcome','uphold','complaint_id',complaint->'id','human_review',true,'review_assessment','The specific warning remains justified after human review'));
 ASSERT changed->>'upheld_measure' IS NULL AND NOT EXISTS(SELECT 1 FROM moderation_holds WHERE case_id=c),'upheld warning created a restriction';
 ASSERT (SELECT disposition_at IS NOT NULL FROM moderation_cases WHERE id=c);
 RAISE NOTICE 'PASS typed replay, independent urgent priority, discoverable unknown notice and field-specific reviewed minimization with remedies preserved';
END $$;
ROLLBACK;

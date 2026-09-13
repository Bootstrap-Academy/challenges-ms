-- Synthetic supported assessment/closure sequence; rolled back after assertions.
-- Only complaint_until is aged to isolate closure after an actual opened event.
BEGIN;
DO $$
DECLARE person uuid:=gen_random_uuid(); actor uuid:=gen_random_uuid(); c uuid:=gen_random_uuid();
 d jsonb; cmd jsonb; first_review uuid:=gen_random_uuid(); second_review uuid:=gen_random_uuid();
 first_due timestamptz:=clock_timestamp()+interval '1 day'; second_due timestamptz:=clock_timestamp()+interval '2 days';
 urgent uuid:=gen_random_uuid(); urgent_due timestamptz; body_before jsonb;
BEGIN
 PERFORM moderation_open(c,actor,'create',person,person,'own_review',NULL,'{"comment":"necessary comment","attachments":["necessary attachment"]}');
 cmd:=jsonb_build_object('case_id',c,'request_key',gen_random_uuid(),'expected_revision',0,'outcome','warn','rationale','Individually reviewed synthetic warning','ground','Synthetic specific ground','rule_version','Synthetic original reference','automation','Human fixture decision','scope','Erstellen von Teilaufgaben auf Bootstrap Academy','redress','Human review and independent remedies');
 d:=moderation_decide(actor,cmd);
 PERFORM moderation_opened(person,id) FROM moderation_messages WHERE case_id=c;
 UPDATE moderation_messages SET complaint_until=clock_timestamp()-interval '1 day' WHERE case_id=c;
 PERFORM moderation_retention(actor,jsonb_build_object('id',first_review,'case_id',c,'action','retain','reason','Specific existing claim still needs this comment','legal_or_claim_basis','Synthetic individually assessed evidence need','necessary_fields',jsonb_build_array('comment'),'review_at',first_due));
 PERFORM moderation_retention(actor,jsonb_build_object('id',second_review,'case_id',c,'action','retain','reason','Independent existing claim needs the attachment','legal_or_claim_basis','Synthetic independent evidence need','necessary_fields',jsonb_build_array('attachments'),'review_at',second_due));
 SELECT public_statement INTO body_before FROM moderation_decisions WHERE id=(d->>'decision_id')::uuid;
 PERFORM moderation_maintenance();PERFORM moderation_maintenance();
 ASSERT (SELECT closed_at IS NOT NULL AND review_due_at=first_due FROM moderation_cases WHERE id=c),'closure lost the outstanding assessment';
 ASSERT (SELECT public_statement=body_before FROM moderation_decisions WHERE id=(d->>'decision_id')::uuid),'scheduling rewrote the original statement';
 FOR i IN 1..100 LOOP PERFORM moderation_open(gen_random_uuid(),actor,'create',person,person,'own_review',NULL,'{}'); END LOOP;
 ASSERT (SELECT row->>'id' FROM jsonb_array_elements(moderation_queue(100,0)) row WHERE row->>'target_id'=person::text LIMIT 1)=c::text,'closed due work lost priority behind its later open work';
 PERFORM moderation_retention(actor,jsonb_build_object('id',gen_random_uuid(),'case_id',c,'action','release_retention','retention_id',first_review,'reason','The specifically reviewed comment is no longer necessary'));
 ASSERT (SELECT closed_at IS NOT NULL AND review_due_at=second_due FROM moderation_cases WHERE id=c),'releasing one assessment lost the independent next review';
 PERFORM moderation_escalate(actor,jsonb_build_object('id',urgent,'case_id',c,'kind','article18_assessment','facts','Specific synthetic urgent facts','assessment','Unresolved urgent human assessment','human_responsibility','Synthetic responsible reviewer'));
 SELECT review_due_at INTO urgent_due FROM moderation_cases WHERE id=c;
 ASSERT urgent_due<second_due AND (SELECT closed_at IS NULL FROM moderation_cases WHERE id=c),'new urgent work must reopen and retain earliest deadline';
 PERFORM moderation_retention(actor,jsonb_build_object('id',gen_random_uuid(),'case_id',c,'action','release_retention','retention_id',second_review,'reason','The specific attachment necessity has now ended'));
 PERFORM moderation_maintenance();
 ASSERT (SELECT review_due_at=urgent_due AND closed_at IS NULL FROM moderation_cases WHERE id=c),'retention release resolved independent urgent work';
 ASSERT (SELECT private_evidence->>'comment'='necessary comment' AND private_evidence ? 'attachments' FROM moderation_cases WHERE id=c),'a due date change automatically deleted evidence';
 RAISE NOTICE 'PASS closed retention priority, two individual releases, urgent reopen, immutable statements and no automatic evidence disposal';
END $$;
ROLLBACK;

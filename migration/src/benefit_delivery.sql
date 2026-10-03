-- Prospective earning facts only. Existing solved rows are never backfilled.
CREATE TABLE challenge_benefit_earnings (
 id uuid PRIMARY KEY, user_id uuid NOT NULL, subtask_id uuid NOT NULL,
 earned_at timestamptz NOT NULL DEFAULT clock_timestamp(), original jsonb NOT NULL,
 UNIQUE(user_id,subtask_id)
);
CREATE TABLE challenge_benefit_components (
 id uuid PRIMARY KEY, earning_id uuid NOT NULL REFERENCES challenge_benefit_earnings(id),
 ordinal integer NOT NULL, user_id uuid NOT NULL, kind text NOT NULL CHECK(kind IN ('xp','coins')),
 request jsonb NOT NULL, state text NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','uncertain','applied','review')),
 attempts integer NOT NULL DEFAULT 0, next_attempt_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 receipt jsonb, UNIQUE(earning_id,ordinal)
);
CREATE INDEX challenge_benefit_due ON challenge_benefit_components(next_attempt_at) WHERE state IN ('pending','uncertain');
CREATE TABLE challenge_benefit_observations (
 component_id uuid NOT NULL REFERENCES challenge_benefit_components(id),
 attempt integer NOT NULL, received_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 result jsonb NOT NULL, PRIMARY KEY(component_id,attempt)
);

CREATE FUNCTION challenge_benefit_original() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_TABLE_NAME IN ('challenge_benefit_earnings','challenge_benefit_observations') OR
   (to_jsonb(NEW)-ARRAY['state','attempts','next_attempt_at','receipt']) IS DISTINCT FROM
   (to_jsonb(OLD)-ARRAY['state','attempts','next_attempt_at','receipt']) THEN
   RAISE EXCEPTION 'Original benefit evidence is immutable';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER challenge_benefit_earning_original BEFORE UPDATE ON challenge_benefit_earnings FOR EACH ROW EXECUTE FUNCTION challenge_benefit_original();
CREATE TRIGGER challenge_benefit_component_original BEFORE UPDATE ON challenge_benefit_components FOR EACH ROW EXECUTE FUNCTION challenge_benefit_original();
CREATE TRIGGER challenge_benefit_observation_original BEFORE UPDATE ON challenge_benefit_observations FOR EACH ROW EXECUTE FUNCTION challenge_benefit_original();

CREATE FUNCTION challenge_benefit_export(p_user uuid) RETURNS jsonb LANGUAGE sql STABLE AS $$
 SELECT jsonb_build_object(
  'earnings',coalesce((SELECT jsonb_agg(to_jsonb(e) ORDER BY e.earned_at,e.id) FROM challenge_benefit_earnings e WHERE e.user_id=p_user),'[]'),
  'components',coalesce((SELECT jsonb_agg(to_jsonb(c) ORDER BY c.earning_id,c.ordinal) FROM challenge_benefit_components c WHERE c.user_id=p_user),'[]'),
  'observations',coalesce((SELECT jsonb_agg(to_jsonb(o) ORDER BY o.component_id,o.attempt) FROM challenge_benefit_observations o JOIN challenge_benefit_components c ON c.id=o.component_id WHERE c.user_id=p_user),'[]'))
$$;

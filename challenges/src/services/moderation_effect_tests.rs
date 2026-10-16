//! Opt-in PostgreSQL regressions against the real migrated, disposable schema.
//! Run with MODERATION_TEST_DATABASE_URL and `cargo test -p challenges
//! moderation::effect_tests:: -- --ignored --nocapture --test-threads=1`.
//! The first fixture stops the clock of its transaction, so a hold can start
//! or end at exactly the compared instant. The second builds realistic holds
//! against the real clock. Everything the tests create rolls back.
use std::collections::{HashMap, HashSet};

use entity::{challenges_subtasks, sea_orm_active_enums::ChallengesSubtaskType};
use sea_orm::{
    ConnectionTrait, Database, DatabaseTransaction, DbBackend, Statement, TransactionTrait,
};
use uuid::Uuid;

use super::{effective_subtask, effective_subtasks, value, EFFECTIVE_SUBTASKS};

const STATES: u32 = 8;
const SCENARIOS: u32 = 33;

/// The statement the list ran first: one call of `moderation_effect` per target.
const REFERENCE: &str = "SELECT coalesce(jsonb_object_agg(id,moderation_effect('subtask',id)),'{}') AS value FROM moderation_targets WHERE kind='subtask' AND id=ANY($1)";

/// The statement the list ran before it read the holds of the list in one
/// pass: an aggregate over the holds of each target through `LATERAL`.
const LATERAL: &str = r#"
SELECT t.id,
       t.base_enabled AND NOT t.withdrawn AND NOT coalesce(h.blocks, false) AS enabled,
       t.base_retired OR coalesce(h.retires, false) AS retired,
       t.withdrawn OR coalesce(h.removes, false) AS removed
FROM moderation_targets t
LEFT JOIN LATERAL (
    SELECT bool_or(effect IN ('hide', 'remove', 'restrict')) AS blocks,
           bool_or(effect = 'retire') AS retires,
           bool_or(effect = 'remove') AS removes
    FROM moderation_holds
    WHERE target_kind = 'subtask' AND target_id = t.id AND active
      AND starts_at <= clock_timestamp()
      AND (ends_at IS NULL OR ends_at > clock_timestamp())
) h ON true
WHERE t.kind = 'subtask' AND t.id = ANY($1)"#;

/// Every trait of a hold that one of the rules looks at, counted over the
/// realistic holds. Each count has to stay above zero at every size.
const COVERAGE: &str = r#"
SELECT jsonb_build_object(
    'hide in force at a subtask', count(*) FILTER (WHERE target_kind = 'subtask' AND in_force AND effect = 'hide'),
    'remove in force at a subtask', count(*) FILTER (WHERE target_kind = 'subtask' AND in_force AND effect = 'remove'),
    'restrict in force at a subtask', count(*) FILTER (WHERE target_kind = 'subtask' AND in_force AND effect = 'restrict'),
    'retire in force at a subtask', count(*) FILTER (WHERE target_kind = 'subtask' AND in_force AND effect = 'retire'),
    'switched off', count(*) FILTER (WHERE target_kind = 'subtask' AND NOT active),
    'ended while still active', count(*) FILTER (WHERE target_kind = 'subtask' AND active AND ends_at <= clock_timestamp()),
    'not started yet', count(*) FILTER (WHERE target_kind = 'subtask' AND active AND starts_at > clock_timestamp()),
    'starting at -infinity', count(*) FILTER (WHERE target_kind = 'subtask' AND in_force AND starts_at = '-infinity'),
    'ending at infinity', count(*) FILTER (WHERE target_kind = 'subtask' AND in_force AND ends_at = 'infinity'),
    'in force at another kind with a subtask id', count(*) FILTER (WHERE target_kind <> 'subtask' AND in_force
        AND EXISTS (SELECT 1 FROM moderation_targets s WHERE s.kind = 'subtask' AND s.id = target_id)),
    'subtasks with several holds in force', (SELECT count(*) FROM (
        SELECT 1 FROM moderation_holds WHERE target_kind = 'subtask' AND active AND starts_at <= clock_timestamp()
          AND (ends_at IS NULL OR ends_at > clock_timestamp()) GROUP BY target_id HAVING count(*) > 1) AS s)
) AS value
FROM (SELECT *, active AND starts_at <= clock_timestamp() AND (ends_at IS NULL OR ends_at > clock_timestamp()) AS in_force
      FROM moderation_holds) AS h"#;

/// The flags of the listed subtask targets as if they had no hold at all.
const WITHOUT_HOLDS: &str = "SELECT id, base_enabled AND NOT withdrawn AS enabled, base_retired AS retired, withdrawn AS removed FROM moderation_targets WHERE kind = 'subtask' AND id = ANY($1)";

type Flags = (bool, bool, bool);

fn target(state: u32, scenario: u32) -> Uuid {
    format!("00000000-0000-4000-8{state:03}-{scenario:012}")
        .parse()
        .unwrap()
}

fn subtask(id: Uuid, enabled: bool, retired: bool, removed: bool) -> challenges_subtasks::Model {
    challenges_subtasks::Model {
        id,
        task_id: Uuid::nil(),
        creator: Uuid::nil(),
        creation_timestamp: Default::default(),
        xp: 0,
        coins: 0,
        enabled,
        ty: ChallengesSubtaskType::Question,
        retired,
        moderation_removed: removed,
    }
}

fn flags(row: &challenges_subtasks::Model) -> Flags {
    (row.enabled, row.retired, row.moderation_removed)
}

/// The flags one of the statements decides for the listed ids, by id. An id
/// must not come back twice.
async fn decided(txn: &DatabaseTransaction, statement: &str, ids: &[Uuid]) -> HashMap<Uuid, Flags> {
    let mut decided = HashMap::new();
    for row in txn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Postgres,
            statement,
            [ids.to_vec().into()],
        ))
        .await
        .unwrap()
    {
        let id: Uuid = row.try_get("", "id").unwrap();
        let state = (
            row.try_get("", "enabled").unwrap(),
            row.try_get("", "retired").unwrap(),
            row.try_get("", "removed").unwrap(),
        );
        assert!(decided.insert(id, state).is_none(), "{id} came back twice");
    }
    decided
}

fn assert_same(what: &str, got: &HashMap<Uuid, Flags>, want: &HashMap<Uuid, Flags>) {
    let wrong: Vec<_> = want
        .iter()
        .filter(|(id, state)| got.get(id) != Some(state))
        .collect();
    assert!(
        wrong.is_empty() && got.len() == want.len(),
        "{what}: {} of {} targets differ, first {:?} got {:?}; {} rows for {} targets",
        wrong.len(),
        want.len(),
        wrong.first(),
        wrong.first().map(|(id, _)| got.get(id)),
        got.len(),
        want.len()
    );
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL via MODERATION_TEST_DATABASE_URL"]
async fn effective_subtasks_match_moderation_effect_postgres() {
    let db = Database::connect(std::env::var("MODERATION_TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let txn = db.begin().await.unwrap();
    txn.execute_unprepared(include_str!("fixtures/moderation_effect.sql"))
        .await
        .unwrap();
    // The personal export calls this projection inside a READ ONLY transaction.
    txn.execute_unprepared("SET TRANSACTION READ ONLY")
        .await
        .unwrap();
    let read_only: String = txn
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT current_setting('transaction_read_only') AS mode",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "mode")
        .unwrap();
    assert_eq!(read_only, "on");

    let targets: Vec<(u32, u32, Uuid)> = (0..STATES)
        .flat_map(|state| {
            (0..SCENARIOS).map(move |scenario| (state, scenario, target(state, scenario)))
        })
        .collect();
    let ids: Vec<Uuid> = targets.iter().map(|(_, _, id)| *id).collect();
    // A subtask without a moderation target keeps whatever its own row says.
    // The last id is known only as a withdrawn target of another kind.
    let other_kind: Uuid = "00000000-0000-4000-c000-000000000001".parse().unwrap();
    let unknown: Vec<_> = (0..8)
        .map(|bits| subtask(Uuid::new_v4(), bits & 1 != 0, bits & 2 != 0, bits & 4 != 0))
        .chain([subtask(other_kind, true, false, false)])
        .collect();
    let asked: Vec<Uuid> = ids
        .iter()
        .copied()
        .chain(unknown.iter().map(|row| row.id))
        .collect();
    let reference = value(&txn, REFERENCE, vec![asked.into()]).await.unwrap();
    assert_eq!(reference.as_object().unwrap().len(), ids.len());
    let expected = |id: Uuid| {
        let state = &reference[id.to_string()];
        (
            state["enabled"].as_bool().unwrap(),
            state["retired"].as_bool().unwrap(),
            state["removed"].as_bool().unwrap(),
        )
    };

    // Every flag of a known target starts as the opposite of its expected
    // value, so a flag which the projection left alone cannot pass. The known
    // targets are listed a second time behind the unknown ones, because every
    // row of a list needs its flags, also when an id occurs more than once.
    let known: Vec<_> = ids
        .iter()
        .map(|&id| {
            let (enabled, retired, removed) = expected(id);
            subtask(id, !enabled, !retired, !removed)
        })
        .collect();
    let rows = effective_subtasks(&txn, [&known[..], &unknown[..], &known[..]].concat())
        .await
        .unwrap();
    assert_eq!(rows.len(), 2 * ids.len() + unknown.len());
    let (first, rest) = rows.split_at(ids.len());
    let (untouched, second) = rest.split_at(unknown.len());
    let mut seen = HashSet::new();
    for (index, row) in first.iter().chain(second).enumerate() {
        let (state, scenario, id) = targets[index % targets.len()];
        assert_eq!(row.id, id);
        assert_eq!(
            flags(row),
            expected(id),
            "target state {state}, hold scenario {scenario}, occurrence {}",
            index / targets.len() + 1
        );
        seen.insert(flags(row));
    }
    assert_eq!(untouched, &unknown[..]);
    // A removed or withdrawn target is never enabled, which leaves six of the
    // eight combinations. The data must reach each of them.
    assert_eq!(seen.len(), 6);
    println!(
        "PASS {} targets, each listed twice, match moderation_effect in a READ ONLY transaction, {} subtasks without a target unchanged",
        ids.len(),
        unknown.len()
    );

    // Fixed expectations keep the comparison honest if the reference itself
    // or the fixture ever stopped saying anything: (enabled, retired, removed).
    for (state, scenario, want) in [
        (0, 0, (true, false, false)),
        (0, 1, (false, false, false)),
        (0, 2, (false, false, true)),
        (0, 3, (false, false, false)),
        (0, 4, (true, true, false)),
        (0, 6, (false, false, true)),
        (0, 10, (true, false, false)),
        (0, 14, (true, false, false)),
        (0, 18, (true, false, false)),
        (0, 22, (false, true, true)),
        (0, 23, (true, true, false)),
        (0, 24, (false, false, false)),
        (0, 25, (false, false, false)),
        (0, 26, (false, true, true)),
        (0, 27, (true, false, false)),
        // The edges: a hold decides from the instant of its start on and no
        // longer at the instant of its stored end.
        (0, 29, (false, false, false)),
        (0, 30, (true, false, false)),
        (0, 31, (true, false, false)),
        (0, 32, (false, false, false)),
        (1, 0, (false, false, false)),
        (2, 0, (true, true, false)),
        (4, 0, (false, false, true)),
        (7, 26, (false, true, true)),
    ] {
        let row = effective_subtask(
            &txn,
            subtask(target(state, scenario), !want.0, !want.1, !want.2),
        )
        .await
        .unwrap();
        assert_eq!(
            flags(&row),
            want,
            "target state {state}, hold scenario {scenario}"
        );
    }
    assert!(effective_subtasks(&txn, Vec::new())
        .await
        .unwrap()
        .is_empty());
    println!("PASS fixed expectations for single targets and an empty list");
    txn.rollback().await.unwrap();
    db.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL via MODERATION_TEST_DATABASE_URL"]
async fn effective_subtasks_match_lateral_statement_on_realistic_holds_postgres() {
    let db = Database::connect(std::env::var("MODERATION_TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let txn = db.begin().await.unwrap();
    txn.execute_unprepared(include_str!("fixtures/moderation_effect_realistic.sql"))
        .await
        .unwrap();
    let listed: Vec<Uuid> = txn
        .query_all(Statement::from_string(
            DbBackend::Postgres,
            "SELECT id FROM realistic_list ORDER BY position",
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get("", "id").unwrap())
        .collect();
    // Subtasks without a subtask target keep whatever their own row says.
    let untouched = (true, false, true);

    // The table grows from a few hundred holds on a few pages, where the
    // planner read the holds per target sequentially for the LATERAL form, to
    // sizes where it reads them through the partial index.
    let mut holds = 0;
    for size in [200, 2_000, 20_000] {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT pg_temp.realistic_holds($1, $2)",
            [(holds + 1).into(), size.into()],
        ))
        .await
        .unwrap();
        holds = size;
        txn.execute_unprepared("ANALYZE moderation_targets, moderation_cases, moderation_holds")
            .await
            .unwrap();
        let coverage = value(&txn, COVERAGE, Vec::new()).await.unwrap();
        for (kind, count) in coverage.as_object().unwrap() {
            assert!(count.as_i64().unwrap() > 0, "{size} holds: none {kind}");
        }

        let reference: HashMap<Uuid, Flags> = value(&txn, REFERENCE, vec![listed.clone().into()])
            .await
            .unwrap()
            .as_object()
            .unwrap()
            .iter()
            .map(|(id, state)| {
                (
                    id.parse().unwrap(),
                    (
                        state["enabled"].as_bool().unwrap(),
                        state["retired"].as_bool().unwrap(),
                        state["removed"].as_bool().unwrap(),
                    ),
                )
            })
            .collect();
        // Holds have to turn each flag of some targets away from what the
        // target alone would give.
        let alone = decided(&txn, WITHOUT_HOLDS, &listed).await;
        let turned = |flag: fn(&Flags) -> bool| {
            reference
                .iter()
                .filter(|(id, state)| flag(state) != flag(&alone[*id]))
                .count()
        };
        let turned = [turned(|s| s.0), turned(|s| s.1), turned(|s| s.2)];
        assert!(
            turned.iter().all(|&count| count > 0),
            "{size} holds: holds turned (enabled, retired, removed) of {turned:?} targets"
        );

        for mode in ["force_custom_plan", "force_generic_plan"] {
            txn.execute_unprepared(&format!("SET LOCAL plan_cache_mode = {mode}"))
                .await
                .unwrap();
            let lateral = decided(&txn, LATERAL, &listed).await;
            assert_same(
                &format!("{size} holds, {mode}, LATERAL against moderation_effect"),
                &lateral,
                &reference,
            );
            let current = decided(&txn, EFFECTIVE_SUBTASKS, &listed).await;
            assert_same(
                &format!("{size} holds, {mode}, statement against LATERAL"),
                &current,
                &lateral,
            );

            // Through the service, every row starts with the opposite flags.
            let rows: Vec<_> = listed
                .iter()
                .map(|&id| match reference.get(&id) {
                    Some(&(enabled, retired, removed)) => subtask(id, !enabled, !retired, !removed),
                    None => subtask(id, untouched.0, untouched.1, untouched.2),
                })
                .collect();
            let rows = effective_subtasks(&txn, rows).await.unwrap();
            assert_eq!(rows.len(), listed.len());
            for (row, &id) in rows.iter().zip(&listed) {
                assert_eq!(row.id, id);
                assert_eq!(
                    flags(row),
                    reference.get(&id).copied().unwrap_or(untouched),
                    "{size} holds, {mode}, effective_subtasks for {id}"
                );
            }
        }
        println!(
            "PASS {size} holds: {} listed rows, {} targets, LATERAL, the statement and effective_subtasks match moderation_effect with custom and generic plans",
            listed.len(),
            reference.len()
        );
    }
    txn.rollback().await.unwrap();
    db.close().await.unwrap();
}

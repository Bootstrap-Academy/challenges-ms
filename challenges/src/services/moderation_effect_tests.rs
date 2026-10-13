//! Opt-in PostgreSQL regression against the real migrated, disposable schema.
//! Run with MODERATION_TEST_DATABASE_URL and `cargo test -p challenges
//! effective_subtasks_match_moderation_effect_postgres -- --ignored --nocapture`.
//! The fixture stops the clock of its transaction, so a hold can start or end
//! at exactly the compared instant. Everything the test creates rolls back.
use std::collections::HashSet;

use entity::{challenges_subtasks, sea_orm_active_enums::ChallengesSubtaskType};
use sea_orm::{ConnectionTrait, Database, DbBackend, Statement, TransactionTrait};
use uuid::Uuid;

use super::{effective_subtask, effective_subtasks, value};

const STATES: u32 = 8;
const SCENARIOS: u32 = 33;

/// The statement the list ran before: one call of `moderation_effect` per target.
const REFERENCE: &str = "SELECT coalesce(jsonb_object_agg(id,moderation_effect('subtask',id)),'{}') AS value FROM moderation_targets WHERE kind='subtask' AND id=ANY($1)";

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

fn flags(row: &challenges_subtasks::Model) -> (bool, bool, bool) {
    (row.enabled, row.retired, row.moderation_removed)
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

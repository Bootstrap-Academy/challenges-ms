//! Export stored definitions through their authorship records. In particular,
//! owning a parent task or submitting a solution is not subtype authorship.

use std::collections::HashMap;

use entity::{
    challenges_challenges, challenges_coding_challenges, challenges_course_tasks,
    challenges_matchings, challenges_multiple_choice_quizes, challenges_questions,
    challenges_subtasks, challenges_tasks,
};
use schemas::challenges::user_export::{
    ChallengeContent, CodingContent, CourseTaskContent, MatchingContent, MultipleChoiceContent,
    QuestionContent, SubtaskContent, TaskContent,
};
use sea_orm::{
    ColumnTrait, DatabaseTransaction, DbErr, EntityTrait, QueryFilter, QuerySelect, QueryTrait,
};
use uuid::Uuid;

pub async fn subtask_content(
    db: &DatabaseTransaction,
    user_id: Uuid,
) -> Result<HashMap<Uuid, SubtaskContent>, DbErr> {
    subtask_content_for(db, user_id, None).await
}

/// The same lossless projection for a single authoritative target in a case.
pub async fn subtask_content_for(
    db: &DatabaseTransaction,
    user_id: Uuid,
    target: Option<Uuid>,
) -> Result<HashMap<Uuid, SubtaskContent>, DbErr> {
    let owned = || {
        let mut query = challenges_subtasks::Entity::find()
            .select_only()
            .column(challenges_subtasks::Column::Id)
            .filter(challenges_subtasks::Column::Creator.eq(user_id));
        if let Some(target) = target {
            query = query.filter(challenges_subtasks::Column::Id.eq(target));
        }
        query.into_query()
    };
    let mut content = HashMap::new();
    for row in challenges_coding_challenges::Entity::find()
        .filter(challenges_coding_challenges::Column::SubtaskId.in_subquery(owned()))
        .all(db)
        .await?
    {
        insert_unique(
            &mut content,
            row.subtask_id,
            SubtaskContent::CodingChallenge(CodingContent {
                description: row.description,
                evaluator: row.evaluator,
                solution_environment: row.solution_environment,
                solution_code: row.solution_code,
                time_limit: row.time_limit,
                memory_limit: row.memory_limit,
                static_tests: row.static_tests,
                random_tests: row.random_tests,
            }),
        )?;
    }
    for row in challenges_questions::Entity::find()
        .filter(challenges_questions::Column::SubtaskId.in_subquery(owned()))
        .all(db)
        .await?
    {
        insert_unique(
            &mut content,
            row.subtask_id,
            SubtaskContent::Question(QuestionContent {
                question: row.question,
                answers: row.answers,
                case_sensitive: row.case_sensitive,
                ascii_letters: row.ascii_letters,
                digits: row.digits,
                punctuation: row.punctuation,
                blocks: row.blocks,
            }),
        )?;
    }
    for row in challenges_multiple_choice_quizes::Entity::find()
        .filter(challenges_multiple_choice_quizes::Column::SubtaskId.in_subquery(owned()))
        .all(db)
        .await?
    {
        let correct_answer_indices = (0..row.answers.len().min(64))
            .filter(|i| row.correct_answers as u64 & (1_u64 << i) != 0)
            .map(|i| i as u8)
            .collect();
        insert_unique(
            &mut content,
            row.subtask_id,
            SubtaskContent::MultipleChoiceQuestion(MultipleChoiceContent {
                question: row.question,
                answers: row.answers,
                correct_answers_bitmask: row.correct_answers.to_string(),
                correct_answer_indices,
                single_choice: row.single_choice,
            }),
        )?;
    }
    for row in challenges_matchings::Entity::find()
        .filter(challenges_matchings::Column::SubtaskId.in_subquery(owned()))
        .all(db)
        .await?
    {
        insert_unique(
            &mut content,
            row.subtask_id,
            SubtaskContent::Matching(MatchingContent {
                left: row.left,
                right: row.right,
                solution: row.solution,
            }),
        )?;
    }
    Ok(content)
}

pub async fn task_content(
    db: &DatabaseTransaction,
    user_id: Uuid,
) -> Result<HashMap<Uuid, TaskContent>, DbErr> {
    let owned = || {
        challenges_tasks::Entity::find()
            .select_only()
            .column(challenges_tasks::Column::Id)
            .filter(challenges_tasks::Column::Creator.eq(user_id))
            .into_query()
    };
    let mut content = HashMap::new();
    for row in challenges_challenges::Entity::find()
        .filter(challenges_challenges::Column::TaskId.in_subquery(owned()))
        .all(db)
        .await?
    {
        insert_unique(
            &mut content,
            row.task_id,
            TaskContent::Challenge(ChallengeContent {
                category_id: row.category_id,
                skill_ids: row.skill_ids,
                title: row.title,
                description: row.description,
            }),
        )?;
    }
    for row in challenges_course_tasks::Entity::find()
        .filter(challenges_course_tasks::Column::TaskId.in_subquery(owned()))
        .all(db)
        .await?
    {
        insert_unique(
            &mut content,
            row.task_id,
            TaskContent::CourseTask(CourseTaskContent {
                course_id: row.course_id,
                section_id: row.section_id,
                lecture_id: row.lecture_id,
            }),
        )?;
    }
    Ok(content)
}

fn insert_unique<T>(map: &mut HashMap<Uuid, T>, id: Uuid, value: T) -> Result<(), DbErr> {
    if map.insert(id, value).is_some() {
        return Err(inconsistent_content());
    }
    Ok(())
}

/// A 500 makes the existing aggregator mark Challenges unavailable and the
/// combined export incomplete. Never return metadata-only success for an
/// inconsistent definition. Do not put source text or user ids in the error.
pub fn inconsistent_content() -> DbErr {
    DbErr::Custom("Authored export has missing or inconsistent subtype records".into())
}

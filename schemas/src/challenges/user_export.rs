//! Schemas of the internal user data export.
//!
//! The export contains everything this service stores about a single user.
//! User ids of other people (the administrator who issued a ban, the users who
//! attempted a subtask the user has created) are left out, so the export never
//! discloses anybody else's data.

use chrono::{DateTime, Utc};
use entity::{
    challenges_ban, challenges_coding_challenge_result, challenges_coding_challenge_submissions,
    challenges_matching_attempts, challenges_multiple_choice_attempts,
    challenges_question_attempts, challenges_subtask_reports, challenges_subtasks,
    challenges_tasks, challenges_user_subtasks,
    sea_orm_active_enums::{
        ChallengesBanAction, ChallengesRating, ChallengesReportReason, ChallengesSubtaskType,
        ChallengesVerdict,
    },
};
use poem_openapi::Object;
use uuid::Uuid;

/// Everything this service stores about a single user.
#[derive(Debug, Clone, Object)]
pub struct UserDataExport {
    /// The progress of the user on the subtasks they have worked on.
    pub subtask_progress: Vec<UserSubtask>,
    /// The attempts of the user at multiple choice questions.
    pub multiple_choice_attempts: Vec<Attempt>,
    /// The attempts of the user at questions.
    pub question_attempts: Vec<Attempt>,
    /// The attempts of the user at matchings.
    pub matching_attempts: Vec<Attempt>,
    /// The solutions the user has submitted for coding challenges, including
    /// the source code and the evaluation result.
    pub coding_challenge_submissions: Vec<Submission>,
    /// The reports the user has submitted about subtasks.
    pub subtask_reports: Vec<SubtaskReport>,
    /// The bans that have been issued against the user.
    pub bans: Vec<Ban>,
    /// The subtasks the user has created.
    pub subtasks_created: Vec<Subtask>,
    /// The tasks the user has created.
    pub tasks_created: Vec<Task>,
}

/// The progress of the user on a single subtask.
#[derive(Debug, Clone, Object)]
pub struct UserSubtask {
    /// The subtask the progress belongs to.
    pub subtask_id: Uuid,
    /// The point in time at which the user solved the subtask.
    pub solved_timestamp: Option<DateTime<Utc>>,
    /// The feedback the user has given for the subtask.
    pub rating: Option<ChallengesRating>,
    /// The point in time at which the user gave the feedback.
    pub rating_timestamp: Option<DateTime<Utc>>,
    /// The point in time of the last attempt of the user.
    pub last_attempt_timestamp: Option<DateTime<Utc>>,
    /// The number of attempts of the user.
    pub attempts: i32,
}

/// A single attempt of the user at a subtask.
#[derive(Debug, Clone, Object)]
pub struct Attempt {
    /// The unique identifier of the attempt.
    pub id: Uuid,
    /// The subtask the attempt belongs to.
    pub subtask_id: Uuid,
    /// The point in time of the attempt.
    pub timestamp: DateTime<Utc>,
    /// Whether the attempt solved the subtask.
    pub solved: bool,
}

/// A solution the user has submitted for a coding challenge.
#[derive(Debug, Clone, Object)]
pub struct Submission {
    /// The unique identifier of the submission.
    pub id: Uuid,
    /// The coding challenge the submission belongs to.
    pub subtask_id: Uuid,
    /// The point in time at which the submission was created.
    pub creation_timestamp: DateTime<Utc>,
    /// The environment the solution was run in.
    pub environment: String,
    /// The source code of the solution.
    pub code: String,
    /// The evaluation result of the submission.
    pub result: Option<SubmissionResult>,
}

/// The evaluation result of a submission.
#[derive(Debug, Clone, Object)]
pub struct SubmissionResult {
    /// The verdict of the evaluation.
    pub verdict: ChallengesVerdict,
    /// The reason for the verdict.
    pub reason: Option<String>,
    /// The exit code of the build step.
    pub build_status: Option<i32>,
    /// The stderr output of the build step.
    pub build_stderr: Option<String>,
    /// The run time of the build step in milliseconds.
    pub build_time: Option<i32>,
    /// The memory usage of the build step in kilobytes.
    pub build_memory: Option<i32>,
    /// The exit code of the run step.
    pub run_status: Option<i32>,
    /// The stderr output of the run step.
    pub run_stderr: Option<String>,
    /// The run time of the run step in milliseconds.
    pub run_time: Option<i32>,
    /// The memory usage of the run step in kilobytes.
    pub run_memory: Option<i32>,
}

/// A report the user has submitted about a subtask.
#[derive(Debug, Clone, Object)]
pub struct SubtaskReport {
    /// The unique identifier of the report.
    pub id: Uuid,
    /// The subtask the report is about.
    pub subtask_id: Uuid,
    /// The point in time at which the report was submitted.
    pub timestamp: DateTime<Utc>,
    /// The reason of the report.
    pub reason: ChallengesReportReason,
    /// The comment of the report.
    pub comment: String,
}

/// A ban that has been issued against the user.
#[derive(Debug, Clone, Object)]
pub struct Ban {
    /// The unique identifier of the ban.
    pub id: Uuid,
    /// The start of the ban.
    pub start: DateTime<Utc>,
    /// The end of the ban, if it is not permanent.
    pub end: Option<DateTime<Utc>>,
    /// The action the user is banned from.
    pub action: ChallengesBanAction,
    /// The reason of the ban.
    pub reason: String,
}

/// A subtask the user has created.
#[derive(Debug, Clone, Object)]
pub struct Subtask {
    /// The unique identifier of the subtask.
    pub id: Uuid,
    /// The parent task.
    pub task_id: Uuid,
    /// The type of the subtask.
    #[oai(rename = "type")]
    pub ty: ChallengesSubtaskType,
    /// The point in time at which the subtask was created.
    pub creation_timestamp: DateTime<Utc>,
    /// The number of xp a user gets for completing this subtask.
    pub xp: i64,
    /// The number of morphcoins a user gets for completing this subtask.
    pub coins: i64,
    /// Whether the subtask is enabled and visible to normal users.
    pub enabled: bool,
    /// Whether the subtask is retired.
    pub retired: bool,
}

/// A task the user has created.
#[derive(Debug, Clone, Object)]
pub struct Task {
    /// The unique identifier of the task.
    pub id: Uuid,
    /// The point in time at which the task was created.
    pub creation_timestamp: DateTime<Utc>,
}

impl From<challenges_user_subtasks::Model> for UserSubtask {
    fn from(value: challenges_user_subtasks::Model) -> Self {
        Self {
            subtask_id: value.subtask_id,
            solved_timestamp: value.solved_timestamp.map(|ts| ts.and_utc()),
            rating: value.rating,
            rating_timestamp: value.rating_timestamp.map(|ts| ts.and_utc()),
            last_attempt_timestamp: value.last_attempt_timestamp.map(|ts| ts.and_utc()),
            attempts: value.attempts,
        }
    }
}

impl From<challenges_multiple_choice_attempts::Model> for Attempt {
    fn from(value: challenges_multiple_choice_attempts::Model) -> Self {
        Self {
            id: value.id,
            subtask_id: value.question_id,
            timestamp: value.timestamp.and_utc(),
            solved: value.solved,
        }
    }
}

impl From<challenges_question_attempts::Model> for Attempt {
    fn from(value: challenges_question_attempts::Model) -> Self {
        Self {
            id: value.id,
            subtask_id: value.question_id,
            timestamp: value.timestamp.and_utc(),
            solved: value.solved,
        }
    }
}

impl From<challenges_matching_attempts::Model> for Attempt {
    fn from(value: challenges_matching_attempts::Model) -> Self {
        Self {
            id: value.id,
            subtask_id: value.matching_id,
            timestamp: value.timestamp.and_utc(),
            solved: value.solved,
        }
    }
}

impl Submission {
    pub fn from(
        submission: challenges_coding_challenge_submissions::Model,
        result: Option<challenges_coding_challenge_result::Model>,
    ) -> Self {
        Self {
            id: submission.id,
            subtask_id: submission.subtask_id,
            creation_timestamp: submission.creation_timestamp.and_utc(),
            environment: submission.environment,
            code: submission.code,
            result: result.map(Into::into),
        }
    }
}

impl From<challenges_coding_challenge_result::Model> for SubmissionResult {
    fn from(value: challenges_coding_challenge_result::Model) -> Self {
        Self {
            verdict: value.verdict,
            reason: value.reason,
            build_status: value.build_status,
            build_stderr: value.build_stderr,
            build_time: value.build_time,
            build_memory: value.build_memory,
            run_status: value.run_status,
            run_stderr: value.run_stderr,
            run_time: value.run_time,
            run_memory: value.run_memory,
        }
    }
}

impl From<challenges_subtask_reports::Model> for SubtaskReport {
    fn from(value: challenges_subtask_reports::Model) -> Self {
        Self {
            id: value.id,
            subtask_id: value.subtask_id,
            timestamp: value.timestamp.and_utc(),
            reason: value.reason,
            comment: value.comment,
        }
    }
}

impl From<challenges_ban::Model> for Ban {
    fn from(value: challenges_ban::Model) -> Self {
        Self {
            id: value.id,
            start: value.start.and_utc(),
            end: value.end.map(|ts| ts.and_utc()),
            action: value.action,
            reason: value.reason,
        }
    }
}

impl From<challenges_subtasks::Model> for Subtask {
    fn from(value: challenges_subtasks::Model) -> Self {
        Self {
            id: value.id,
            task_id: value.task_id,
            ty: value.ty,
            creation_timestamp: value.creation_timestamp.and_utc(),
            xp: value.xp,
            coins: value.coins,
            enabled: value.enabled,
            retired: value.retired,
        }
    }
}

impl From<challenges_tasks::Model> for Task {
    fn from(value: challenges_tasks::Model) -> Self {
        Self {
            id: value.id,
            creation_timestamp: value.creation_timestamp.and_utc(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use poem_openapi::types::ToJSON;

    use super::*;

    fn timestamp() -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 9, 3)
            .unwrap()
            .and_hms_opt(12, 34, 56)
            .unwrap()
    }

    #[test]
    fn user_subtask_does_not_contain_the_user_id() {
        let user_id = Uuid::new_v4();
        let subtask_id = Uuid::new_v4();

        let exported = UserSubtask::from(challenges_user_subtasks::Model {
            user_id,
            subtask_id,
            solved_timestamp: Some(timestamp()),
            rating: Some(ChallengesRating::Positive),
            rating_timestamp: Some(timestamp()),
            last_attempt_timestamp: Some(timestamp()),
            attempts: 3,
        });

        assert_eq!(exported.subtask_id, subtask_id);
        assert_eq!(exported.attempts, 3);
        assert_eq!(exported.solved_timestamp, Some(timestamp().and_utc()));
        assert!(!exported.to_json_string().contains(&user_id.to_string()));
    }

    #[test]
    fn attempts_keep_the_subtask_of_every_subtask_type() {
        let user_id = Uuid::new_v4();
        let id = Uuid::new_v4();
        let subtask_id = Uuid::new_v4();

        let multiple_choice = Attempt::from(challenges_multiple_choice_attempts::Model {
            id,
            question_id: subtask_id,
            user_id,
            timestamp: timestamp(),
            solved: true,
        });
        let question = Attempt::from(challenges_question_attempts::Model {
            id,
            question_id: subtask_id,
            user_id,
            timestamp: timestamp(),
            solved: true,
        });
        let matching = Attempt::from(challenges_matching_attempts::Model {
            id,
            matching_id: subtask_id,
            user_id,
            timestamp: timestamp(),
            solved: true,
        });

        for attempt in [multiple_choice, question, matching] {
            assert_eq!(attempt.id, id);
            assert_eq!(attempt.subtask_id, subtask_id);
            assert_eq!(attempt.timestamp, timestamp().and_utc());
            assert!(attempt.solved);
            assert!(!attempt.to_json_string().contains(&user_id.to_string()));
        }
    }

    #[test]
    fn submission_contains_the_code_and_the_result() {
        let creator = Uuid::new_v4();
        let id = Uuid::new_v4();
        let subtask_id = Uuid::new_v4();

        let exported = Submission::from(
            challenges_coding_challenge_submissions::Model {
                id,
                subtask_id,
                creator,
                creation_timestamp: timestamp(),
                environment: "python".into(),
                code: "print(42)".into(),
            },
            Some(challenges_coding_challenge_result::Model {
                submission_id: id,
                verdict: ChallengesVerdict::Ok,
                reason: Some("reason".into()),
                build_status: Some(0),
                build_stderr: Some("build stderr".into()),
                build_time: Some(1),
                build_memory: Some(2),
                run_status: Some(0),
                run_stderr: Some("run stderr".into()),
                run_time: Some(3),
                run_memory: Some(4),
            }),
        );

        assert_eq!(exported.code, "print(42)");
        assert_eq!(exported.environment, "python");
        let result = exported.result.clone().unwrap();
        assert_eq!(result.reason.as_deref(), Some("reason"));
        assert_eq!(result.run_stderr.as_deref(), Some("run stderr"));
        assert!(!exported.to_json_string().contains(&creator.to_string()));
    }

    #[test]
    fn submission_without_a_result() {
        let exported = Submission::from(
            challenges_coding_challenge_submissions::Model {
                id: Uuid::new_v4(),
                subtask_id: Uuid::new_v4(),
                creator: Uuid::new_v4(),
                creation_timestamp: timestamp(),
                environment: "python".into(),
                code: "print(42)".into(),
            },
            None,
        );

        assert!(exported.result.is_none());
    }

    #[test]
    fn subtask_report_does_not_contain_the_user_id() {
        let user_id = Uuid::new_v4();
        let subtask_id = Uuid::new_v4();

        let exported = SubtaskReport::from(challenges_subtask_reports::Model {
            id: Uuid::new_v4(),
            subtask_id,
            user_id: Some(user_id),
            timestamp: timestamp(),
            reason: ChallengesReportReason::Wrong,
            comment: "comment".into(),
        });

        assert_eq!(exported.subtask_id, subtask_id);
        assert_eq!(exported.comment, "comment");
        assert!(!exported.to_json_string().contains(&user_id.to_string()));
    }

    #[test]
    fn ban_does_not_contain_the_administrator_who_issued_it() {
        let user_id = Uuid::new_v4();
        let creator = Uuid::new_v4();

        let exported = Ban::from(challenges_ban::Model {
            id: Uuid::new_v4(),
            user_id,
            start: timestamp(),
            end: None,
            action: ChallengesBanAction::Create,
            creator,
            reason: "reason".into(),
        });

        assert_eq!(exported.start, timestamp().and_utc());
        assert_eq!(exported.end, None);
        assert_eq!(exported.reason, "reason");
        let json = exported.to_json_string();
        assert!(!json.contains(&creator.to_string()));
        assert!(!json.contains(&user_id.to_string()));
    }

    #[test]
    fn created_content_does_not_contain_the_creator() {
        let creator = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        let subtask = Subtask::from(challenges_subtasks::Model {
            id: Uuid::new_v4(),
            task_id,
            creator,
            creation_timestamp: timestamp(),
            xp: 10,
            coins: 20,
            enabled: true,
            ty: ChallengesSubtaskType::Question,
            retired: false,
        });
        let task = Task::from(challenges_tasks::Model {
            id: task_id,
            creator,
            creation_timestamp: timestamp(),
        });

        assert_eq!(subtask.task_id, task_id);
        assert_eq!(subtask.xp, 10);
        assert_eq!(subtask.coins, 20);
        assert_eq!(task.creation_timestamp, timestamp().and_utc());
        assert!(!subtask.to_json_string().contains(&creator.to_string()));
        assert!(!task.to_json_string().contains(&creator.to_string()));
    }
}

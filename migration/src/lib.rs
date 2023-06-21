#![forbid(unsafe_code)]
#![warn(clippy::dbg_macro, clippy::use_debug, clippy::todo)]

pub use sea_orm_migration::prelude::*;

pub struct Migrator;

mod m20230322_163425_challenges_init;
mod m20230326_074819_multiple_choice_attempts;
mod m20230330_174101_challenges_skills;
mod m20230505_153021_coding_challenges;
mod m20230522_183823_remove_examples;
mod m20230522_192000_add_cc_solution;
mod m20230525_065623_cc_number_of_tests;
mod m20230527_151432_cc_submissions;
mod m20230612_182959_new_course_tasks;
mod m20230618_144250_add_fee_to_subtasks;
mod m20230618_150706_add_unlocked_subtasks;
mod m20230619_084345_user_subtasks;
mod m20230620_082405_subtask_feedback;
mod m20230620_093716_reports;
mod m20230620_153019_dislike_reports;
mod m20230620_163944_ban;
mod m20230620_221620_pre_check_failed_verdict;
mod m20230621_072201_single_choice;
mod m20230621_074711_questions;
mod m20230621_120013_fix_foreign_keys;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20230322_163425_challenges_init::Migration),
            Box::new(m20230326_074819_multiple_choice_attempts::Migration),
            Box::new(m20230330_174101_challenges_skills::Migration),
            Box::new(m20230505_153021_coding_challenges::Migration),
            Box::new(m20230522_183823_remove_examples::Migration),
            Box::new(m20230522_192000_add_cc_solution::Migration),
            Box::new(m20230525_065623_cc_number_of_tests::Migration),
            Box::new(m20230527_151432_cc_submissions::Migration),
            Box::new(m20230612_182959_new_course_tasks::Migration),
            Box::new(m20230618_144250_add_fee_to_subtasks::Migration),
            Box::new(m20230618_150706_add_unlocked_subtasks::Migration),
            Box::new(m20230619_084345_user_subtasks::Migration),
            Box::new(m20230620_082405_subtask_feedback::Migration),
            Box::new(m20230620_093716_reports::Migration),
            Box::new(m20230620_153019_dislike_reports::Migration),
            Box::new(m20230620_163944_ban::Migration),
            Box::new(m20230620_221620_pre_check_failed_verdict::Migration),
            Box::new(m20230621_072201_single_choice::Migration),
            Box::new(m20230621_074711_questions::Migration),
            Box::new(m20230621_120013_fix_foreign_keys::Migration),
        ]
    }
}

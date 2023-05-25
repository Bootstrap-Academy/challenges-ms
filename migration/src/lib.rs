#![forbid(unsafe_code)]
#![warn(clippy::dbg_macro, clippy::use_debug, clippy::todo)]

pub use sea_orm_migration::prelude::*;

pub struct Migrator;

// mod m20230204_171617_create_companies_table;
// mod m20230204_171643_create_jobs_table;
// mod m20230204_180119_create_skill_requirements_table;
mod m20230322_163425_challenges_init;
mod m20230326_074819_multiple_choice_attempts;
mod m20230330_174101_challenges_skills;
mod m20230505_153021_coding_challenges;
mod m20230522_183823_remove_examples;
mod m20230522_192000_add_cc_solution;
mod m20230525_065623_cc_number_of_tests;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            // Box::new(m20230204_171617_create_companies_table::Migration),
            // Box::new(m20230204_171643_create_jobs_table::Migration),
            // Box::new(m20230204_180119_create_skill_requirements_table::Migration),
            Box::new(m20230322_163425_challenges_init::Migration),
            Box::new(m20230326_074819_multiple_choice_attempts::Migration),
            Box::new(m20230330_174101_challenges_skills::Migration),
            Box::new(m20230505_153021_coding_challenges::Migration),
            Box::new(m20230522_183823_remove_examples::Migration),
            Box::new(m20230522_192000_add_cc_solution::Migration),
            Box::new(m20230525_065623_cc_number_of_tests::Migration),
        ]
    }
}

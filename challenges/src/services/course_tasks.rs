use lib::services::{ServiceResult, Services};

pub async fn get_skills_of_course(
    services: &Services,
    course_id: &str,
) -> ServiceResult<Vec<String>> {
    Ok(services
        .skills
        .get_skills()
        .await?
        .into_iter()
        .filter(|(_, skill)| skill.courses.iter().any(|x| x == course_id))
        .map(|(x, _)| x)
        .collect())
}

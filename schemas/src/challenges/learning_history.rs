use poem_openapi::Object;
use uuid::Uuid;

/// An exact historical course lecture, supplied by another trusted service.
#[derive(Debug, Object)]
pub struct LectureBinding {
    pub course_id: String,
    pub lecture_id: String,
}

#[derive(Debug, Object)]
pub struct LearningHistoryRequest {
    #[oai(default, validator(max_items = 500))]
    pub subtask_ids: Vec<Uuid>,
    #[oai(default, validator(max_items = 500))]
    pub lecture_bindings: Vec<LectureBinding>,
}

/// Participation only: no answers, code, solutions or success claims.
#[derive(Debug, Default, Object)]
pub struct LearningHistory {
    pub attempted_subtask_ids: Vec<Uuid>,
    pub attempted_lecture_bindings: Vec<LectureBinding>,
}

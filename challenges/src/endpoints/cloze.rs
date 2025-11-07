use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
};

use chrono::{DateTime, Utc};
use entity::{
    challenges_cloze_attempts, challenges_cloze_blanks, challenges_cloze_options,
    challenges_clozes, challenges_user_subtasks, sea_orm_active_enums::ChallengesSubtaskType,
};
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    config::Config,
    SharedState,
};
use poem::web::Data;
use poem_ext::{db::DbTxn, response};
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    OpenApi,
};
use schemas::challenges::{
    cloze::{
        Cloze, ClozeSummary, ClozeVariant, ClozeWithSolution, CreateClozeBlank, CreateClozeRequest,
        SolveClozeFeedback, SolveClozeRequest, UpdateClozeRequest,
    },
    subtasks::Subtask,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, DbErr, EntityTrait, QueryFilter,
    QueryOrder, Set, Unchanged,
};
use tracing::warn;
use uuid::Uuid;

use super::Tags;
use crate::services::subtasks::{
    create_subtask, deduct_hearts, get_subtask, get_user_subtask, query_subtask,
    query_subtask_admin, query_subtasks, send_task_rewards, update_subtask, update_user_subtask,
    CreateSubtaskError, QuerySubtaskAdminError, QuerySubtasksFilter, UpdateSubtaskError,
    UserSubtaskExt,
};

pub struct Clozes {
    pub state: Arc<SharedState>,
    pub config: Arc<Config>,
}

#[OpenApi(tag = "Tags::Cloze")]
impl Clozes {
    #[oai(path = "/tasks/:task_id/clozes", method = "get")]
    #[allow(clippy::too_many_arguments)]
    async fn list_clozes(
        &self,
        task_id: Path<Uuid>,
        attempted: Query<Option<bool>>,
        solved: Query<Option<bool>>,
        rated: Query<Option<bool>>,
        enabled: Query<Option<bool>>,
        retired: Query<Option<bool>>,
        creator: Query<Option<Uuid>>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> ListClozes::Response<VerifiedUserAuth> {
        let entries = query_subtasks::<challenges_clozes::Entity, _>(
            &db,
            &auth.0,
            task_id.0,
            QuerySubtasksFilter {
                attempted: attempted.0,
                solved: solved.0,
                rated: rated.0,
                enabled: enabled.0,
                retired: retired.0,
                creator: creator.0,
                ty: Some(ChallengesSubtaskType::Cloze),
            },
            |specific, subtask| (specific, subtask),
        )
        .await?;

        let hydrated = hydrate_many(&***db, entries).await?;
        ListClozes::ok(
            hydrated
                .into_iter()
                .map(|item| {
                    ClozeSummary::from(&item.cloze, item.subtask, &item.blanks, &item.options)
                })
                .collect(),
        )
    }

    #[oai(path = "/tasks/:task_id/clozes/:subtask_id", method = "get")]
    async fn get_cloze(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetCloze::Response<VerifiedUserAuth> {
        match query_subtask::<challenges_clozes::Entity, _>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            |specific, subtask| (specific, subtask),
        )
        .await?
        {
            Some(item) => {
                let hydrated = hydrate_single(&***db, item).await?;
                GetCloze::ok(Cloze::from(
                    &hydrated.cloze,
                    hydrated.subtask,
                    &hydrated.blanks,
                    &hydrated.options,
                ))
            }
            None => GetCloze::subtask_not_found(),
        }
    }

    #[oai(path = "/tasks/:task_id/clozes/:subtask_id/solution", method = "get")]
    async fn get_cloze_with_solution(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetClozeWithSolution::Response<VerifiedUserAuth> {
        match query_subtask_admin::<challenges_clozes::Entity, _>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            |specific, subtask| (specific, subtask),
        )
        .await?
        {
            Ok(item) => {
                let hydrated = hydrate_single(&***db, item).await?;
                GetClozeWithSolution::ok(ClozeWithSolution::from(
                    &hydrated.cloze,
                    hydrated.subtask,
                    &hydrated.blanks,
                    &hydrated.options,
                ))
            }
            Err(QuerySubtaskAdminError::NotFound) => GetClozeWithSolution::subtask_not_found(),
            Err(QuerySubtaskAdminError::NoAccess) => GetClozeWithSolution::forbidden(),
        }
    }

    #[oai(path = "/tasks/:task_id/clozes", method = "post")]
    async fn create_cloze(
        &self,
        task_id: Path<Uuid>,
        data: Json<CreateClozeRequest>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> CreateCloze::Response<VerifiedUserAuth> {
        let Json(payload) = data;
        let CreateClozeRequest {
            subtask,
            content,
            blanks,
            options,
            case_sensitive,
        } = payload;

        let prepared = match PreparedCloze::new(content, case_sensitive, blanks, options) {
            Ok(value) => value,
            Err(err) => return CreateCloze::invalid_payload(err.to_string()),
        };

        let subtask = match create_subtask(
            &db,
            &self.state.services,
            &self.config,
            &auth.0,
            task_id.0,
            subtask,
            ChallengesSubtaskType::Cloze,
        )
        .await?
        {
            Ok(subtask) => subtask,
            Err(CreateSubtaskError::TaskNotFound) => return CreateCloze::task_not_found(),
            Err(CreateSubtaskError::Forbidden) => return CreateCloze::forbidden(),
            Err(CreateSubtaskError::Banned(until)) => return CreateCloze::banned(until),
            Err(CreateSubtaskError::XpLimitExceeded(limit)) => {
                return CreateCloze::xp_limit_exceeded(limit)
            }
            Err(CreateSubtaskError::CoinLimitExceeded(limit)) => {
                return CreateCloze::coin_limit_exceeded(limit)
            }
        };

        let cloze = challenges_clozes::ActiveModel {
            subtask_id: Set(subtask.id),
            content: Set(prepared.content.clone()),
            case_sensitive: Set(prepared.case_sensitive),
        }
        .insert(&***db)
        .await?;

        persist_prepared(&***db, cloze.subtask_id, &prepared).await?;

        let hydrated = hydrate_single(&***db, (cloze, subtask)).await?;
        CreateCloze::ok(ClozeWithSolution::from(
            &hydrated.cloze,
            hydrated.subtask,
            &hydrated.blanks,
            &hydrated.options,
        ))
    }

    #[oai(path = "/tasks/:task_id/clozes/:subtask_id", method = "patch")]
    async fn update_cloze(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        data: Json<UpdateClozeRequest>,
        db: Data<&DbTxn>,
        auth: AdminAuth,
    ) -> UpdateCloze::Response<AdminAuth> {
        let Json(payload) = data;
        let (cloze, subtask) = match update_subtask::<challenges_clozes::Entity>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            payload.subtask,
        )
        .await?
        {
            Ok(value) => value,
            Err(UpdateSubtaskError::SubtaskNotFound) => return UpdateCloze::subtask_not_found(),
            Err(UpdateSubtaskError::TaskNotFound) => return UpdateCloze::task_not_found(),
        };

        let hydrated = hydrate_single(&***db, (cloze.clone(), subtask.clone())).await?;
        let option_positions: HashMap<Uuid, usize> = hydrated
            .options
            .iter()
            .enumerate()
            .map(|(idx, option)| (option.id, idx))
            .collect();

        let existing_blanks: Vec<CreateClozeBlank> = hydrated
            .blanks
            .iter()
            .map(|blank| CreateClozeBlank {
                placeholder: blank.placeholder as u32,
                answer: blank.answer.clone(),
                synonyms: blank.synonyms.clone(),
                option_index: blank
                    .correct_option_id
                    .and_then(|id| option_positions.get(&id).map(|idx| *idx as u32)),
            })
            .collect();
        let existing_options: Vec<String> = hydrated
            .options
            .iter()
            .map(|option| option.label.clone())
            .collect();

        let content = payload.content.get_new(&cloze.content).clone();
        let case_sensitive = *payload.case_sensitive.get_new(&cloze.case_sensitive);
        let blanks = payload.blanks.get_new(&existing_blanks).clone();
        let options = payload.options.get_new(&existing_options).clone();

        let payload_changed = content != cloze.content
            || case_sensitive != cloze.case_sensitive
            || blanks != existing_blanks
            || options != existing_options;

        if !payload_changed {
            return UpdateCloze::ok(ClozeWithSolution::from(
                &hydrated.cloze,
                hydrated.subtask,
                &hydrated.blanks,
                &hydrated.options,
            ));
        }

        let prepared = match PreparedCloze::new(content, case_sensitive, blanks, options) {
            Ok(value) => value,
            Err(err) => return UpdateCloze::invalid_payload(err.to_string()),
        };

        let updated = challenges_clozes::ActiveModel {
            subtask_id: Unchanged(cloze.subtask_id),
            content: Set(prepared.content.clone()),
            case_sensitive: Set(prepared.case_sensitive),
        }
        .update(&***db)
        .await?;

        reset_payload(&***db, cloze.subtask_id).await?;
        persist_prepared(&***db, cloze.subtask_id, &prepared).await?;

        let hydrated = hydrate_single(&***db, (updated, subtask)).await?;
        UpdateCloze::ok(ClozeWithSolution::from(
            &hydrated.cloze,
            hydrated.subtask,
            &hydrated.blanks,
            &hydrated.options,
        ))
    }

    #[oai(path = "/tasks/:task_id/clozes/:subtask_id/attempts", method = "post")]
    async fn solve_cloze(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        data: Json<SolveClozeRequest>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> SolveCloze::Response<VerifiedUserAuth> {
        let Some((cloze, subtask)) =
            get_subtask::<challenges_clozes::Entity>(&db, task_id.0, subtask_id.0).await?
        else {
            return SolveCloze::subtask_not_found();
        };

        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return SolveCloze::subtask_not_found();
        }

        let (blanks, options) = load_payload(&***db, cloze.subtask_id).await?;
        if data.0.answers.len() != blanks.len() {
            return SolveCloze::invalid_payload(format!(
                "Expected {} answers, got {}.",
                blanks.len(),
                data.0.answers.len()
            ));
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        let solved_previously = user_subtask.is_solved();

        if let Some(last_attempt) = user_subtask.last_attempt() {
            let wait = self.config.challenges.clozes.timeout as i64
                - (Utc::now() - last_attempt).num_seconds();
            if wait > 0 {
                return SolveCloze::too_many_requests(wait as u64);
            }
        }

        let variant = if options.is_empty() {
            ClozeVariant::TypeIn
        } else {
            ClozeVariant::Options
        };

        let blanks_by_id: HashMap<Uuid, &challenges_cloze_blanks::Model> =
            blanks.iter().map(|blank| (blank.id, blank)).collect();
        let options_by_id: HashMap<Uuid, &challenges_cloze_options::Model> =
            options.iter().map(|option| (option.id, option)).collect();
        let mut remaining: HashSet<Uuid> = blanks_by_id.keys().copied().collect();
        let mut used_options = HashSet::new();
        let mut correct = 0u32;

        for answer in &data.0.answers {
            let Some(blank) = blanks_by_id.get(&answer.blank_id) else {
                return SolveCloze::invalid_payload(format!(
                    "Unknown blank id {}.",
                    answer.blank_id
                ));
            };

            if !remaining.remove(&answer.blank_id) {
                return SolveCloze::invalid_payload(format!(
                    "Blank {} provided multiple times.",
                    answer.blank_id
                ));
            }

            let success = match variant {
                ClozeVariant::TypeIn => {
                    if answer.option_id.is_some() {
                        return SolveCloze::invalid_payload(
                            "option_id is not allowed for type-in clozes.".into(),
                        );
                    }
                    let text = answer.text.as_deref().unwrap_or("").trim();
                    if text.is_empty() {
                        false
                    } else {
                        let normalized = normalize_answer(text, cloze.case_sensitive);
                        let mut matches =
                            normalize_answer(&blank.answer, cloze.case_sensitive) == normalized;
                        if !matches {
                            matches = blank.synonyms.iter().any(|syn| {
                                normalize_answer(syn, cloze.case_sensitive) == normalized
                            });
                        }
                        matches
                    }
                }
                ClozeVariant::Options => {
                    let Some(option_id) = answer.option_id else {
                        return SolveCloze::invalid_payload(
                            "option_id must be provided for option-based clozes.".into(),
                        );
                    };
                    if answer.text.is_some() {
                        return SolveCloze::invalid_payload(
                            "text answers are not allowed for option-based clozes.".into(),
                        );
                    }
                    if !options_by_id.contains_key(&option_id) {
                        return SolveCloze::invalid_payload(format!(
                            "Option {} does not belong to this cloze.",
                            option_id
                        ));
                    }
                    if !used_options.insert(option_id) {
                        return SolveCloze::invalid_payload(format!(
                            "Option {} was used more than once.",
                            option_id
                        ));
                    }
                    blank.correct_option_id == Some(option_id)
                }
            };

            if success {
                correct += 1;
            }
        }

        if !remaining.is_empty() {
            return SolveCloze::invalid_payload("One or more blanks were not answered.".into());
        }

        let solved = correct as usize == blanks.len();

        if !deduct_hearts(&self.state.services, &self.config, &auth.0, &subtask).await? {
            return SolveCloze::not_enough_hearts();
        }

        if !solved_previously {
            let now = Utc::now().naive_utc();
            if solved {
                update_user_subtask(
                    &db,
                    user_subtask.as_ref(),
                    challenges_user_subtasks::ActiveModel {
                        user_id: Set(auth.0.id),
                        subtask_id: Set(subtask.id),
                        solved_timestamp: Set(Some(now)),
                        last_attempt_timestamp: Set(Some(now)),
                        attempts: Set(user_subtask.attempts() as i32 + 1),
                        ..Default::default()
                    },
                )
                .await?;

                if auth.0.id != subtask.creator {
                    send_task_rewards(&self.state.services, &db, auth.0.id, &subtask).await?;
                }
            } else {
                update_user_subtask(
                    &db,
                    user_subtask.as_ref(),
                    challenges_user_subtasks::ActiveModel {
                        user_id: Set(auth.0.id),
                        subtask_id: Set(subtask.id),
                        last_attempt_timestamp: Set(Some(now)),
                        attempts: Set(user_subtask.attempts() as i32 + 1),
                        ..Default::default()
                    },
                )
                .await?;
            }

            challenges_cloze_attempts::ActiveModel {
                id: Set(Uuid::new_v4()),
                cloze_id: Set(cloze.subtask_id),
                user_id: Set(auth.0.id),
                timestamp: Set(now),
                correct: Set(correct as i32),
                total: Set(blanks.len() as i32),
                solved: Set(solved),
            }
            .insert(&***db)
            .await?;
        }

        SolveCloze::ok(SolveClozeFeedback {
            solved,
            correct,
            total: blanks.len() as u32,
        })
    }
}

response!(ListClozes = {
    Ok(200) => Vec<ClozeSummary>,
});

response!(GetCloze = {
    Ok(200) => Cloze,
    SubtaskNotFound(404, error),
});

response!(GetClozeWithSolution = {
    Ok(200) => ClozeWithSolution,
    SubtaskNotFound(404, error),
    Forbidden(403, error),
});

response!(CreateCloze = {
    Ok(201) => ClozeWithSolution,
    TaskNotFound(404, error),
    Forbidden(403, error),
    Banned(403, error) => Option<DateTime<Utc>>,
    XpLimitExceeded(403, error) => u64,
    CoinLimitExceeded(403, error) => u64,
    InvalidPayload(400, error) => String,
});

response!(UpdateCloze = {
    Ok(200) => ClozeWithSolution,
    SubtaskNotFound(404, error),
    TaskNotFound(404, error),
    InvalidPayload(400, error) => String,
});

response!(SolveCloze = {
    Ok(201) => SolveClozeFeedback,
    TooManyRequests(429, error) => u64,
    SubtaskNotFound(404, error),
    NotEnoughHearts(403, error),
    InvalidPayload(400, error) => String,
});

struct HydratedCloze {
    cloze: challenges_clozes::Model,
    subtask: Subtask,
    blanks: Vec<challenges_cloze_blanks::Model>,
    options: Vec<challenges_cloze_options::Model>,
}

async fn hydrate_many(
    db: &DatabaseTransaction,
    entries: Vec<(challenges_clozes::Model, Subtask)>,
) -> Result<Vec<HydratedCloze>, DbErr> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = entries.iter().map(|(cloze, _)| cloze.subtask_id).collect();

    let mut blanks = challenges_cloze_blanks::Entity::find()
        .filter(challenges_cloze_blanks::Column::ClozeId.is_in(ids.clone()))
        .order_by_asc(challenges_cloze_blanks::Column::Placeholder)
        .all(db)
        .await?;

    let mut options = challenges_cloze_options::Entity::find()
        .filter(challenges_cloze_options::Column::ClozeId.is_in(ids.clone()))
        .order_by_asc(challenges_cloze_options::Column::Position)
        .all(db)
        .await?;

    blanks.sort_by_key(|blank| (blank.cloze_id, blank.placeholder, blank.id));
    options.sort_by_key(|option| (option.cloze_id, option.position, option.id));

    let mut blanks_by_cloze: HashMap<Uuid, Vec<challenges_cloze_blanks::Model>> = HashMap::new();
    for blank in blanks {
        blanks_by_cloze
            .entry(blank.cloze_id)
            .or_default()
            .push(blank);
    }

    let mut options_by_cloze: HashMap<Uuid, Vec<challenges_cloze_options::Model>> = HashMap::new();
    for option in options {
        options_by_cloze
            .entry(option.cloze_id)
            .or_default()
            .push(option);
    }

    Ok(entries
        .into_iter()
        .map(|(cloze, subtask)| HydratedCloze {
            blanks: blanks_by_cloze
                .remove(&cloze.subtask_id)
                .unwrap_or_default(),
            options: options_by_cloze
                .remove(&cloze.subtask_id)
                .unwrap_or_default(),
            cloze,
            subtask,
        })
        .collect())
}

async fn hydrate_single(
    db: &DatabaseTransaction,
    item: (challenges_clozes::Model, Subtask),
) -> Result<HydratedCloze, DbErr> {
    let mut hydrated = hydrate_many(db, vec![item]).await?;
    Ok(hydrated.remove(0))
}

async fn load_payload(
    db: &DatabaseTransaction,
    cloze_id: Uuid,
) -> Result<
    (
        Vec<challenges_cloze_blanks::Model>,
        Vec<challenges_cloze_options::Model>,
    ),
    DbErr,
> {
    let blanks = challenges_cloze_blanks::Entity::find()
        .filter(challenges_cloze_blanks::Column::ClozeId.eq(cloze_id))
        .order_by_asc(challenges_cloze_blanks::Column::Placeholder)
        .order_by_asc(challenges_cloze_blanks::Column::Id)
        .all(db)
        .await?;

    let options = challenges_cloze_options::Entity::find()
        .filter(challenges_cloze_options::Column::ClozeId.eq(cloze_id))
        .order_by_asc(challenges_cloze_options::Column::Position)
        .order_by_asc(challenges_cloze_options::Column::Id)
        .all(db)
        .await?;

    Ok((blanks, options))
}

#[derive(Debug)]
struct PreparedCloze {
    content: String,
    case_sensitive: bool,
    blanks: Vec<PreparedBlank>,
    options: Vec<String>,
}

#[derive(Debug)]
struct PreparedBlank {
    placeholder: u32,
    answer: String,
    synonyms: Vec<String>,
    option_index: Option<usize>,
}

impl PreparedCloze {
    fn new(
        content: String,
        case_sensitive: bool,
        blanks: Vec<CreateClozeBlank>,
        options: Vec<String>,
    ) -> Result<Self, ValidationError> {
        let placeholders = extract_placeholders(&content)?;
        if placeholders.is_empty() {
            return Err(ValidationError::NoPlaceholders);
        }

        let mut defs: HashMap<u32, CreateClozeBlank> = HashMap::new();
        for blank in blanks {
            let placeholder = blank.placeholder;
            if defs.insert(placeholder, blank).is_some() {
                return Err(ValidationError::DuplicatePlaceholder(placeholder));
            }
        }

        let mut sanitized_options = Vec::with_capacity(options.len());
        let mut option_labels = HashSet::with_capacity(options.len());
        for option in options {
            let label = option.trim().to_string();
            if label.is_empty() {
                return Err(ValidationError::EmptyOption);
            }
            if !option_labels.insert(label.clone()) {
                return Err(ValidationError::DuplicateOption(label));
            }
            sanitized_options.push(label);
        }

        let variant_b = !sanitized_options.is_empty();
        if variant_b && sanitized_options.len() < placeholders.len() {
            return Err(ValidationError::TooFewOptions {
                blanks: placeholders.len(),
                options: sanitized_options.len(),
            });
        }

        let mut used_option_indices = HashSet::new();
        let mut prepared_blanks = Vec::with_capacity(placeholders.len());

        for placeholder in &placeholders {
            let Some(def) = defs.remove(placeholder) else {
                return Err(ValidationError::MissingDefinition(*placeholder));
            };

            if def.answer.trim().is_empty() {
                return Err(ValidationError::EmptyAnswer(*placeholder));
            }
            let answer = def.answer;
            let synonyms = sanitize_synonyms(&def.synonyms, case_sensitive);

            let option_index = match (variant_b, def.option_index) {
                (true, Some(idx)) => {
                    let idx = idx as usize;
                    if idx >= sanitized_options.len() {
                        return Err(ValidationError::OptionOutOfRange {
                            placeholder: *placeholder,
                            index: idx,
                        });
                    }
                    if !used_option_indices.insert(idx) {
                        return Err(ValidationError::OptionAlreadyUsed(idx));
                    }
                    Some(idx)
                }
                (true, None) => return Err(ValidationError::MissingOptionAssignment(*placeholder)),
                (false, Some(_)) => {
                    return Err(ValidationError::UnexpectedOptionAssignment(*placeholder))
                }
                (false, None) => None,
            };

            prepared_blanks.push(PreparedBlank {
                placeholder: *placeholder,
                answer,
                synonyms,
                option_index,
            });
        }

        if !defs.is_empty() {
            let trimmed: Vec<u32> = defs.keys().copied().collect();
            warn!(
                placeholders = ?trimmed,
                "Ignoring blank definitions without matching placeholders"
            );
        }

        prepared_blanks.sort_by_key(|blank| blank.placeholder);

        Ok(Self {
            content,
            case_sensitive,
            blanks: prepared_blanks,
            options: sanitized_options,
        })
    }
}

fn sanitize_synonyms(values: &[String], case_sensitive: bool) -> Vec<String> {
    let mut unique = HashSet::new();
    let mut sanitized = Vec::new();
    for value in values {
        if value.trim().is_empty() {
            continue;
        }
        let normalized = normalize_answer(value, case_sensitive);
        if normalized.is_empty() {
            continue;
        }
        if unique.insert(normalized) {
            sanitized.push(value.clone());
        }
    }
    sanitized
}

fn normalize_answer(value: &str, case_sensitive: bool) -> String {
    let mut out = String::with_capacity(value.len());
    let mut whitespace = false;
    for ch in value.trim().chars() {
        if ch.is_whitespace() {
            if !whitespace {
                out.push(' ');
            }
            whitespace = true;
        } else {
            whitespace = false;
            if case_sensitive {
                out.push(ch);
            } else {
                out.push(ch.to_ascii_lowercase());
            }
        }
    }
    out
}

fn extract_placeholders(content: &str) -> Result<Vec<u32>, ValidationError> {
    let mut placeholders = Vec::new();
    let mut seen = HashSet::new();
    let mut remaining = content;
    while let Some(idx) = remaining.find("{{blank_") {
        let start = idx + 8;
        let after = &remaining[start..];
        let mut digits = String::new();
        for ch in after.chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
            } else {
                break;
            }
        }
        if digits.is_empty() {
            return Err(ValidationError::InvalidPlaceholder);
        }
        let suffix = &after[digits.len()..];
        if !suffix.starts_with("}}") {
            return Err(ValidationError::InvalidPlaceholder);
        }
        let placeholder: u32 = digits
            .parse()
            .map_err(|_| ValidationError::InvalidPlaceholder)?;
        if placeholder == 0 {
            return Err(ValidationError::InvalidPlaceholder);
        }
        if !seen.insert(placeholder) {
            return Err(ValidationError::DuplicatePlaceholder(placeholder));
        }
        placeholders.push(placeholder);
        remaining = &suffix[2..];
    }
    Ok(placeholders)
}

async fn persist_prepared(
    db: &DatabaseTransaction,
    cloze_id: Uuid,
    prepared: &PreparedCloze,
) -> Result<(), DbErr> {
    let mut option_ids = Vec::with_capacity(prepared.options.len());
    for (idx, label) in prepared.options.iter().enumerate() {
        let id = Uuid::new_v4();
        challenges_cloze_options::ActiveModel {
            id: Set(id),
            cloze_id: Set(cloze_id),
            position: Set(idx as i32),
            label: Set(label.clone()),
        }
        .insert(db)
        .await?;
        option_ids.push(id);
    }

    for blank in &prepared.blanks {
        challenges_cloze_blanks::ActiveModel {
            id: Set(Uuid::new_v4()),
            cloze_id: Set(cloze_id),
            placeholder: Set(blank.placeholder as i32),
            answer: Set(blank.answer.clone()),
            synonyms: Set(blank.synonyms.clone()),
            correct_option_id: Set(blank.option_index.map(|idx| option_ids[idx])),
        }
        .insert(db)
        .await?;
    }

    Ok(())
}

async fn reset_payload(db: &DatabaseTransaction, cloze_id: Uuid) -> Result<(), DbErr> {
    challenges_cloze_blanks::Entity::delete_many()
        .filter(challenges_cloze_blanks::Column::ClozeId.eq(cloze_id))
        .exec(db)
        .await?;
    challenges_cloze_options::Entity::delete_many()
        .filter(challenges_cloze_options::Column::ClozeId.eq(cloze_id))
        .exec(db)
        .await?;
    challenges_cloze_attempts::Entity::delete_many()
        .filter(challenges_cloze_attempts::Column::ClozeId.eq(cloze_id))
        .exec(db)
        .await?;
    Ok(())
}

#[derive(Debug)]
enum ValidationError {
    NoPlaceholders,
    InvalidPlaceholder,
    DuplicatePlaceholder(u32),
    MissingDefinition(u32),
    EmptyAnswer(u32),
    EmptyOption,
    DuplicateOption(String),
    TooFewOptions { blanks: usize, options: usize },
    MissingOptionAssignment(u32),
    UnexpectedOptionAssignment(u32),
    OptionOutOfRange { placeholder: u32, index: usize },
    OptionAlreadyUsed(usize),
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPlaceholders => write!(f, "Content must contain at least one placeholder."),
            Self::InvalidPlaceholder => write!(
                f,
                "Placeholders must follow the form {{blank_<number>}} with positive numbers."
            ),
            Self::DuplicatePlaceholder(idx) => write!(
                f,
                "Placeholder {{blank_{idx}}} appears more than once (in the content or definitions)."
            ),
            Self::MissingDefinition(idx) => write!(
                f,
                "Placeholder {{blank_{idx}}} is missing a corresponding definition."
            ),
            Self::EmptyAnswer(idx) => write!(
                f,
                "Placeholder {{blank_{idx}}} must have a non-empty answer."
            ),
            Self::EmptyOption => write!(f, "Option labels cannot be empty."),
            Self::DuplicateOption(label) => write!(f, "Option \"{label}\" is duplicated."),
            Self::TooFewOptions { blanks, options } => write!(
                f,
                "Received {options} options for {blanks} blanks. Variant B requires at least as many options as blanks."
            ),
            Self::MissingOptionAssignment(idx) => write!(
                f,
                "Placeholder {{blank_{idx}}} requires an option assignment when options exist."
            ),
            Self::UnexpectedOptionAssignment(idx) => write!(
                f,
                "Placeholder {{blank_{idx}}} references an option even though no options were provided."
            ),
            Self::OptionOutOfRange { placeholder, index } => write!(
                f,
                "Placeholder {{blank_{placeholder}}} references option index {index}, which is out of range."
            ),
            Self::OptionAlreadyUsed(index) => {
                write!(f, "Option index {index} is assigned to multiple blanks.")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_placeholders() {
        let content = "Fill {{blank_1}} and {{blank_2}}.";
        assert_eq!(extract_placeholders(content).unwrap(), vec![1, 2]);
    }

    #[test]
    fn normalizes_answers() {
        assert_eq!(normalize_answer(" Foo  bar ", false), "foo bar");
        assert_eq!(normalize_answer(" Foo  bar ", true), "Foo bar");
    }

    #[test]
    fn prepared_rejects_duplicate_placeholder_definitions() {
        let blank = CreateClozeBlank {
            placeholder: 1,
            answer: "A".into(),
            synonyms: vec![],
            option_index: None,
        };
        let err = PreparedCloze::new(
            "{{blank_1}}".into(),
            false,
            vec![blank.clone(), blank],
            vec![],
        )
        .unwrap_err();
        assert!(matches!(err, ValidationError::DuplicatePlaceholder(1)));
    }

    #[test]
    fn prepared_requires_option_assignments_when_options_exist() {
        let blank = CreateClozeBlank {
            placeholder: 1,
            answer: "A".into(),
            synonyms: vec![],
            option_index: None,
        };
        let err = PreparedCloze::new("{{blank_1}}".into(), false, vec![blank], vec!["A".into()])
            .unwrap_err();
        assert!(matches!(err, ValidationError::MissingOptionAssignment(1)));
    }
}

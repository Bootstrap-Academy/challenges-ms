//! All three public rankings share one fresh authority and a fixed projection.
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use fnct::{format::JsonFormatter, key};
use lib::{
    auth::User,
    services::{
        publications::{PublicationEpoch, PublicationSnapshot},
        Services,
    },
    Cache,
};
use poem::{http::header, Endpoint, IntoResponse, Middleware, Request, Response};
use poem_ext::responses::{InnerResponse, MetaResponsesExt};
use poem_openapi::ApiResponse;
use schemas::challenges::leaderboard::{
    PublishedLeaderboard, PublishedLeaderboardUser, PublishedRank, PublishedUser,
};
use sea_orm::{
    sea_query::{Expr, SelectStatement},
    DatabaseTransaction,
};
use serde::Serialize;
use uuid::Uuid;

use super::{language, rank_of, ranked_rows, task, user_score};

#[derive(Debug, Clone, Serialize)]
pub enum Ranking {
    Global,
    Task(Uuid),
    Language(String),
}

impl Ranking {
    fn query(&self) -> Option<SelectStatement> {
        match self {
            Self::Global => None,
            Self::Task(id) => Some(task::get_base_query(*id)),
            Self::Language(language) => Some(language::get_base_query(language)),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Unavailable,
    NotFound,
    Changed,
    Unverified,
}

impl Error {
    pub fn response<T: ApiResponse + IntoResponse, A: MetaResponsesExt + Send>(
        self,
    ) -> poem_ext::responses::Response<T, A> {
        let (status, error) = match self {
            Self::Unavailable => (503, "leaderboard_unavailable"),
            Self::NotFound => (404, "not_found"),
            Self::Changed => (409, "publication_changed"),
            Self::Unverified => (403, "unverified"),
        };
        let response = poem::Response::builder()
            .status(poem::http::StatusCode::from_u16(status).unwrap())
            .content_type("application/json")
            .body(serde_json::json!({"error": error}).to_string());
        no_store(Ok(InnerResponse::from_parse_request_error(
            poem::Error::from_response(response),
        )))
    }
}

pub fn no_store<T: ApiResponse + IntoResponse, A: MetaResponsesExt + Send>(
    response: poem_ext::responses::Response<T, A>,
) -> poem_ext::responses::Response<T, A> {
    let mut http = match response {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    };
    http.headers_mut()
        .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    http.headers_mut()
        .insert(header::VARY, "Authorization".parse().unwrap());
    Ok(InnerResponse::from_parse_request_error(
        poem::Error::from_response(http),
    ))
}

/// Covers authentication and parameter errors which precede the route body.
pub struct Headers(pub bool);
pub struct HeaderEndpoint<E> {
    inner: E,
    enabled: bool,
}
impl<E: Endpoint> Middleware<E> for Headers {
    type Output = HeaderEndpoint<E>;
    fn transform(&self, inner: E) -> Self::Output {
        HeaderEndpoint {
            inner,
            enabled: self.0,
        }
    }
}
impl<E: Endpoint> Endpoint for HeaderEndpoint<E> {
    type Output = Response;
    async fn call(&self, request: Request) -> poem::Result<Response> {
        let protect = self.enabled && request.uri().path().starts_with("/leaderboard");
        let mut response = match self.inner.call(request).await {
            Ok(response) => response.into_response(),
            Err(error) if protect => error.into_response(),
            Err(error) => return Err(error),
        };
        if protect {
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
            response
                .headers_mut()
                .insert(header::VARY, "Authorization".parse().unwrap());
        }
        Ok(response)
    }
}

/// A missing authority never permits a legacy public fallback, even on a
/// local rollback. Only an explicit, fresh never-activated policy selects it.
pub async fn use_shared(services: &Services, enabled: bool) -> Result<bool, Error> {
    let epoch = services
        .auth
        .publication_epoch()
        .await
        .map_err(|_| Error::Unavailable)?;
    if !epoch.policy_active && !enabled {
        return Ok(false);
    }
    epoch
        .require_shared(enabled)
        .map_err(|_| Error::Unavailable)?;
    Ok(true)
}

fn participants(mut query: SelectStatement, snapshot: &PublicationSnapshot) -> SelectStatement {
    // A single UUID-array bind avoids a query/RPC per person and parameter
    // count limits; an empty array is an empty comparison set.
    query.and_where(Expr::cust_with_values(
        "\"user_id\" = ANY($1::uuid[])",
        [snapshot.ids()],
    ));
    query
}

async fn fresh_snapshot(
    services: &Services,
    expected: Option<Uuid>,
) -> Result<PublicationSnapshot, Error> {
    for _ in 0..2 {
        let before = services
            .auth
            .publication_epoch()
            .await
            .map_err(|_| Error::Unavailable)?;
        before
            .require_shared(true)
            .map_err(|_| Error::Unavailable)?;
        if expected.is_some_and(|value| value != before.publication_epoch) {
            return Err(Error::Changed);
        }
        let snapshot = services
            .auth
            .publication_snapshot()
            .await
            .map_err(|_| Error::Unavailable)?;
        if before == snapshot.epoch {
            return Ok(snapshot);
        }
    }
    Err(Error::Unavailable)
}

async fn still_current(services: &Services, epoch: &PublicationEpoch) -> Result<bool, Error> {
    Ok(services
        .auth
        .publication_epoch()
        .await
        .map_err(|_| Error::Unavailable)?
        == *epoch)
}

pub async fn leaderboard(
    db: &DatabaseTransaction,
    services: &Services,
    cache: &Cache<JsonFormatter>,
    ranking: Ranking,
    limit: u64,
    offset: u64,
    expected: Option<Uuid>,
) -> Result<PublishedLeaderboard, Error> {
    for _ in 0..2 {
        let snapshot = fresh_snapshot(services, expected).await?;
        let (rows, total) = if let Some(query) = ranking.query() {
            ranked_rows(db, participants(query, &snapshot), limit, offset, true)
                .await
                .map_err(|_| Error::Unavailable)?
        } else {
            match services
                .skills
                .published_leaderboard(limit, offset, &snapshot.epoch)
                .await
            {
                Ok(page) => {
                    if page.scope_version != snapshot.epoch.scope_version
                        || page.publication_epoch != snapshot.epoch.publication_epoch
                        || page.epoch_revision != snapshot.epoch.epoch_revision
                        || page.leaderboard.len() as u64 > limit
                        || page.total < page.leaderboard.len() as u64
                    {
                        return Err(Error::Unavailable);
                    }
                    (
                        page.leaderboard
                            .into_iter()
                            .map(|row| (row.user, row.rank.into()))
                            .collect(),
                        page.total,
                    )
                }
                Err(_) if !still_current(services, &snapshot.epoch).await? => continue,
                Err(_) => return Err(Error::Unavailable),
            }
        };
        let people: HashMap<_, _> = snapshot
            .participants
            .iter()
            .map(|person| (person.user_id, person))
            .collect();
        let mut ids = HashSet::new();
        if rows
            .iter()
            .any(|(id, rank)| !people.contains_key(id) || !ids.insert(*id) || rank.rank == 0)
        {
            return Err(Error::Unavailable);
        }
        // Scores are freshly evaluated. Including their content in the key
        // invalidates score/total changes immediately without cross-service
        // cache signals; epoch binds membership and authoritative names.
        let result = cache
            .cached_result(
                key!(
                    "priv01-ranking-v1",
                    &snapshot.epoch.scope_version,
                    snapshot.epoch.publication_epoch,
                    snapshot.epoch.epoch_revision,
                    &ranking,
                    limit,
                    offset,
                    &rows,
                    total
                ),
                &[],
                Some(Duration::from_secs(10)),
                || async {
                    anyhow::Ok(PublishedLeaderboard {
                        leaderboard: rows
                            .iter()
                            .map(|(id, rank)| PublishedLeaderboardUser {
                                user: PublishedUser {
                                    display_name: people[id].display_name.clone(),
                                    avatar_url: None,
                                },
                                rank: rank.clone(),
                            })
                            .collect(),
                        total,
                        scope_version: snapshot.epoch.scope_version.clone(),
                        publication_epoch: snapshot.epoch.publication_epoch,
                        epoch_revision: snapshot.epoch.epoch_revision,
                    })
                },
            )
            .await
            .map_err(|_| Error::Unavailable)?
            .map_err(|_| Error::Unavailable)?;
        if still_current(services, &snapshot.epoch).await? {
            return Ok(result);
        }
    }
    Err(Error::Unavailable)
}

pub async fn rank(
    db: &DatabaseTransaction,
    services: &Services,
    ranking: Ranking,
    user_id: Uuid,
    viewer: &User,
) -> Result<PublishedRank, Error> {
    if !viewer.email_verified {
        return Err(Error::Unverified);
    }
    for _ in 0..2 {
        let snapshot = fresh_snapshot(services, None).await?;
        let shared = snapshot.participant(user_id).is_some();
        if !shared && viewer.id != user_id {
            // Administrators use their private support APIs, never a public
            // rank response which changes shape according to admin status.
            if still_current(services, &snapshot.epoch).await? {
                return Err(Error::NotFound);
            }
            continue;
        }
        let (score, public_rank) = if let Some(query) = ranking.query() {
            let score = user_score(db, query.clone(), user_id)
                .await
                .map_err(|_| Error::Unavailable)?;
            let rank = if shared {
                match score {
                    Some(score) => Some(
                        rank_of(db, participants(query, &snapshot), score)
                            .await
                            .map_err(|_| Error::Unavailable)?,
                    ),
                    None => None,
                }
            } else {
                None
            };
            (score.unwrap_or(0) as u64, rank)
        } else {
            match services
                .skills
                .published_rank(user_id, &snapshot.epoch)
                .await
            {
                Ok(rank) => {
                    if rank.scope_version != snapshot.epoch.scope_version
                        || rank.publication_epoch != snapshot.epoch.publication_epoch
                        || rank.epoch_revision != snapshot.epoch.epoch_revision
                        || rank.rank != rank.public_rank
                        || (!shared && rank.public_rank.is_some())
                        || rank.public_rank == Some(0)
                    {
                        return Err(Error::Unavailable);
                    }
                    (rank.xp, rank.public_rank)
                }
                Err(_) if !still_current(services, &snapshot.epoch).await? => continue,
                Err(_) => return Err(Error::Unavailable),
            }
        };
        if still_current(services, &snapshot.epoch).await? {
            return Ok(PublishedRank {
                score,
                rank: public_rank,
                public_rank,
                scope_version: snapshot.epoch.scope_version,
                publication_epoch: snapshot.epoch.publication_epoch,
                epoch_revision: snapshot.epoch.epoch_revision,
            });
        }
    }
    Err(Error::Unavailable)
}

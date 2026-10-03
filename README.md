[![check](https://github.com/Bootstrap-Academy/challenges-ms/actions/workflows/check.yml/badge.svg)](https://github.com/Bootstrap-Academy/challenges-ms/actions/workflows/check.yml)
[![test](https://github.com/Bootstrap-Academy/challenges-ms/actions/workflows/test.yml/badge.svg)](https://github.com/Bootstrap-Academy/challenges-ms/actions/workflows/test.yml)
[![build](https://github.com/Bootstrap-Academy/challenges-ms/actions/workflows/build.yml/badge.svg)](https://github.com/Bootstrap-Academy/challenges-ms/actions/workflows/build.yml) <!--
https://app.codecov.io/gh/Bootstrap-Academy/challenges-ms/settings/badge
[![codecov](https://codecov.io/gh/Bootstrap-Academy/challenges-ms/branch/develop/graph/badge.svg?token=changeme)](https://codecov.io/gh/Bootstrap-Academy/challenges-ms) -->
![Version](https://img.shields.io/github/v/tag/Bootstrap-Academy/challenges-ms?include_prereleases&label=version)
[![dependency status](https://deps.rs/repo/github/Bootstrap-Academy/challenges-ms/status.svg)](https://deps.rs/repo/github/Bootstrap-Academy/challenges-ms)

# Bootstrap Academy Challenges Microservice
The official challenges microservice of [Bootstrap Academy](https://bootstrap.academy/).

If you would like to submit a bug report or feature request, or are looking for general information about the project or the publicly available instances, please refer to the [Bootstrap-Academy repository](https://github.com/Bootstrap-Academy/Bootstrap-Academy).

## Development Setup
1. Install the [Rust](https://www.rust-lang.org/) stable toolchain.
2. Clone this repository and `cd` into it.
3. Install [Just](https://github.com/casey/just) (`cargo install just`) and [Sea-ORM](https://www.sea-ql.org/SeaORM/) (`cargo install sea-orm-cli`)
4. Start a [PostgreSQL](https://www.postgresql.org/) database, for example using [Docker](https://www.docker.com/) or [Podman](https://podman.io/):
    ```bash
    podman run -d --rm \
        --name postgres \
        -p 127.0.0.1:5432:5432 \
        -e POSTGRES_HOST_AUTH_METHOD=trust \
        postgres:alpine
    ```
5. Create the `academy-challenges` database:
    ```bash
    podman exec postgres \
        psql -U postgres \
        -c 'create database "academy-challenges"'
    ```
6. Start a [Redis](https://redis.io/) instance, for example using [Docker](https://www.docker.com/) or [Podman](https://podman.io/):
    ```bash
    podman run -d --rm \
        --name redis \
        -p 127.0.0.1:6379:6379 \
        redis:alpine
    ```
7. Run `just migrate` to run the database migrations.
8. Run `just run` to start the microservice. You can find the automatically generated swagger documentation on http://localhost:8005/docs.

## Coding execution

After running migrations, `challenges` starts the API and its coding worker as before. To scale them separately, run `challenges api` and one or more `challenges worker` processes with the same database and service configuration. Workers do not bind an HTTP port. Set `challenges.coding_challenges.execution.embedded_worker = false` to make the default command serve only the API instead. The existing `max_concurrency` setting limits execution slots per worker process.

The optional `[challenges.coding_challenges.execution]` configuration defaults to:

```toml
embedded_worker = true
max_pending = 1024
max_pending_per_user = 4
lease_seconds = 30
poll_milliseconds = 500
retry_seconds = 10
max_execution_seconds = 600
```

Use consistent admission limits across API replicas. Pending includes running and technically deferred submissions; an overflowing queue responds with the existing HTTP 429 retry response before storing a submission or charging a heart. Previously accepted work remains queued even if it exceeds a newly configured limit. Technical executor failures stay pending and retry without charging hearts.

PostgreSQL claims only committed submissions and fences result/progress/outbox commits by lease owner and generation. Heartbeats renew a live lease every third of its duration. The queue API reports live advertised execution capacity, actual leased submissions, and waiting submissions across processes. Position zero means actively leased; positive positions are an approximate waiting order and may change during retries or concurrent work.

Stop or drain **all old workers before starting this version's workers**: older binaries do not participate in leases. Apply the additive migration before the new binary. During recovery stop all new workers before starting the previous binary and retain the additive schema. Existing submission IDs, results, heart operations and benefit outboxes are preserved. If a process or connection fails, remote sandbox work can physically run again after lease expiry; fencing prevents an obsolete worker from committing results or rewards, but does not promise exactly-once remote computation.

Local database regression (fresh migrated disposable PostgreSQL plus isolated Redis, no live services):

```bash
HEART_TEST_DATABASE_URL=postgresql://... HEART_TEST_REDIS_URL=redis://... \
  cargo test --locked -p challenges coding_durable_execution_postgres -- --ignored --test-threads=1
```

Run that test separately from the historical heart fixtures, which intentionally leave submission history behind. `cargo test --locked --workspace` runs the regular checks without external fixture dependencies.

For the complete local migration/heart/authority/export regression and two real worker processes against a deliberately stalled local executor, use Python 3.11+, the PostgreSQL/Redis tools on `PATH`, and cached Cargo dependencies:

```bash
python3 scripts/test-coding-execution.py --output /tmp/coding-execution-check
```

The output directory must be new. The script binds all fixtures to loopback, keeps its logs, and stops/removes only its own processes and database cluster.

## Learning access and daily lessons

Attempt admission reads the current Backend policy from
`GET /shop/_internal/learning-policy/{user_id}`. `legacy` and `shadow` retain
the existing heart rules; `daily` bypasses both the balance requirement and
new wrong-answer heart operations. Existing admin, author and retired-task
exceptions remain. Missing users, unknown modes, invalid responses and service
failures return an error before accepting an attempt.

Skills owns course access, lesson binding resolution and the daily counter.
Challenges supplies authenticated user IDs, task/subtask IDs and course,
section and lecture bindings from its database to
`POST /skills/_internal/learning-access/{user_id}/check` or `/start`.
Individual read/code recovery routes only check access. Quiz answers, coding
submissions and running a coding example start learning before accepting work.

Metadata/statistics list admission is opt-in through `challenges.learning_access_reads`
(or `CHALLENGES__LEARNING_ACCESS_READS=true`), default `false`. Full-content
question, matching, multiple-choice and coding lists always check the concrete
course/lesson rights of their detail routes, including with that switch off.
Deploy the Skills `check-batch` route before this Challenges version. Content
and active metadata lists load parent bindings once and check batches of
at most 250 concrete subtasks through
`POST /skills/_internal/learning-access/{user_id}/check-batch`; concrete IDs
preserve lesson-specific rights, including during policy outages. Course-task
lists check each parent binding once. These checks never start a lesson.
The attempt/submission UUID is the internal request ID; Skills' durable
user/course/lesson start makes retrying an uncertain admission safe. Existing
public challenge POSTs keep their original attempt semantics.

The same checks apply to raw exercise IDs and retained-learning routes.
Inaccessible rows are omitted from lists and details keep their existing 404
envelopes. Start refusals preserve Skills' status and JSON, including
`429 daily_limit_reached` with the current `daily` status. Provider errors do
not become incorrect answers or grant access. Shared and standalone exercises
are resolved by Skills; an inaccessible optional binding must not suppress an
otherwise eligible exercise. Course/section-wide bindings keep nullable
lecture IDs, so they cannot masquerade as standalone work.

Heart outbox settlement always submits the original operation to Backend.
The final `daily_learning` receipt requires zero charged half-hearts. Already
completed operations replay their original receipt, including a charge made
before a policy transition. Technical errors and malformed receipts remain
pending; accepted learner results and reward idempotency are unchanged.

Deploy the compatible Backend policy/receipt API and Skills check/start API
before this consumer. Backend's receipt CHECK migration must precede daily
activation. Backend defaults and cohort/terms eligibility control learning
policy; Skills independently controls daily measurement/enforcement. This
consumer does not activate either policy or introduce new terms or prices.

Skills reads historical participation through the service-local
`POST /_internal/users/{user_id}/learning-history`, authenticated with the
existing internal JWT audience `challenges` (including per-audience secrets).
The body is `{subtask_ids: UUID[], lecture_bindings: [{course_id, lecture_id}]}`;
both lists default to empty, and the combined limit is 500 entries before
deduplication. Invalid or oversized requests return 422. The response contains
only `{attempted_subtask_ids: UUID[], attempted_lecture_bindings: [{course_id,
lecture_id}]}`, with unique results restricted to the requested IDs and exact
course/lecture pairs. An unknown user or empty request returns empty lists.

This read-only local query recognizes wrong and solved quiz attempts, submitted
code including pending judgments, and saved progress with an attempt count or
last-attempt/solved timestamp. Empty or rating-only progress, views and broad
course/section bindings do not start a lecture. Historical example executions
have no durable user participation record here and cannot be reconstructed from
shared evaluator caches. No code, answers, solutions or success claims leave
this endpoint, and it does not call Skills or modify XP, receipts or progress.
Deploy this endpoint before the Skills consumer that uses its evidence to
preserve historical lesson starts. The two services may call each other during
admission, but this history lookup has no upstream dependency or subject lock.

Run the native access, heart, historical-authority and coding queue regressions
with cached Cargo dependencies and PostgreSQL/Redis tools on `PATH`:

```bash
python3 scripts/test-learning-access.py --output /tmp/learning-access-check
```

The runner uses fresh disposable databases and local HTTP contract fixtures,
records command results, and stops/removes its own services. It supports
`CARGO_TARGET_DIR`. This tests the Challenges boundary; Skills counter/day
concurrency and real multi-service release acceptance are separate checks.

## Account Deletion
When an account is deleted, the auth microservice calls `DELETE /_internal/users/:user_id` on this microservice.
The endpoint requires an internal token with the `challenges` audience and answers `204`, also for a user that has no data here, so it can be retried safely.

It deletes the bans the user issued or received, their subtask reports, their multiple choice, question and matching attempts, their coding challenge submissions and their user subtask rows, as well as the subtasks they created — including everything referencing those subtasks, which the database removes through `ON DELETE CASCADE`.
Tasks are shared between users, so a task the user created is only deleted once no subtask is left in it.
The cached values tagged with the user id are dropped afterwards.

Because the auth microservice logs and swallows a failing call, a periodic sweep catches the deletions that were lost:

```bash
challenges sweep-deleted-users   # or `cargo run -- sweep-deleted-users` in the dev setup
```

It walks the distinct user ids referenced anywhere in the database in batches, asks the auth microservice for each one and deletes the data of every user that no longer exists there.
The settings live in the `[deleted_user_sweep]` section of `config.toml` and can be overridden with environment variables (`__` separates the section from the property):

| Property | Environment variable | Default | Description |
| --- | --- | --- | --- |
| `batch_size` | `DELETED_USER_SWEEP__BATCH_SIZE` | `500` | Number of user ids loaded from the database per batch. |
| `rate_limit` | `DELETED_USER_SWEEP__RATE_LIMIT` | `10` | Auth microservice requests per second; `0` means unlimited. |

The base url of the auth microservice is `services.auth` (`SERVICES__AUTH`).

In the NixOS module the sweep is a oneshot service with a timer, enabled through `academy.backend.challenges.sweepDeletedUsers.enable` (`interval`, default `daily`, and `randomizedDelay`, default `5m`).

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

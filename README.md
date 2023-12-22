[![check](https://github.com/Bootstrap-Academy/challenges-ms/actions/workflows/check.yml/badge.svg)](https://github.com/Bootstrap-Academy/challenges-ms/actions/workflows/check.yml)
[![test](https://github.com/Bootstrap-Academy/challenges-ms/actions/workflows/test.yml/badge.svg)](https://github.com/Bootstrap-Academy/challenges-ms/actions/workflows/test.yml)
https://app.codecov.io/gh/Bootstrap-Academy/challenges-ms/settings/badge
[![codecov](https://codecov.io/gh/Bootstrap-Academy/challenges-ms/branch/develop/graph/badge.svg?token=changeme)](https://codecov.io/gh/Bootstrap-Academy/challenges-ms) -->
<!-- ![Version](https://img.shields.io/github/v/tag/Bootstrap-Academy/challenges-ms?include_prereleases&label=version) -->
<!-- [![dependency status](https://deps.rs/repo/github/Bootstrap-Academy/challenges-ms/status.svg)](https://deps.rs/repo/github/Bootstrap-Academy/challenges-ms) -->

# Bootstrap Academy Challenges Microservice
The official challenges microservice of [Bootstrap Academy](https://bootstrap.academy/).

If you would like to submit a bug report or feature request, or are looking for general information about the project or the publicly available instances, please refer to the [Bootstrap-Academy repository](https://github.com/Bootstrap-Academy/Bootstrap-Academy).

## Development Setup
1. Install the [Rust](https://www.rust-lang.org/) stable toolchain and [just](https://github.com/casey/just).
2. Clone this repository and `cd` into it.
3. Start a [PostgreSQL](https://www.postgresql.org/) database, for example using [Docker](https://www.docker.com/) or [Podman](https://podman.io/):
    ```bash
    podman run -d --rm \
        --name postgres \
        -p 127.0.0.1:5432:5432 \
        -e POSTGRES_HOST_AUTH_METHOD=trust \
        postgres:alpine
    ```
4. Create the `academy-challenges` database:
    ```bash
    podman exec postgres \
        psql -U postgres \
        -c 'create database "academy-challenges"'
    ```
5. Start a [Redis](https://redis.io/) instance, for example using [Docker](https://www.docker.com/) or [Podman](https://podman.io/):
    ```bash
    podman run -d --rm \
        --name redis \
        -p 127.0.0.1:6379:6379 \
        redis:alpine
    ```
6. Run `just migrate` to run the database migrations.
7. Run `just run` to start the microservice. You can find the automatically generated swagger documentation on http://localhost:8005/docs.

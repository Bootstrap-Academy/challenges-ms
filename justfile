alias r := run
alias c := check
alias t := test
alias p := psql
alias m := migrate
alias e := entity
alias b := bacon

_default:
    @just --list

run *args:
    cargo run --locked {{args}}

check *args:
    cargo clippy {{args}}

test *args:
    cargo test --locked {{args}}

psql *args:
    psql "$(tomlq -r .database.url < .config.toml)" {{args}}

redis ms *args:
    redis-cli -u "$(tomlq -r .redis.{{ms}} < .config.toml)" {{args}}

db:
    docker run -it --rm --name academy-db \
        -e POSTGRES_DB=academy \
        -e POSTGRES_USER=academy \
        -e POSTGRES_PASSWORD=academy \
        -p 127.0.0.1:5432:5432 \
        postgres:alpine

migrate *args:
    sea migrate {{args}}

entity:
    DATABASE_URL="$(tomlq -r .database.url < .config.toml)" sea generate entity -l -o entity/src --with-copy-enums
    sed -i -E 's/^(#\[derive\(.*DeriveActiveEnum.*)\)\]$/\1, poem_openapi::Enum)]/' entity/src/sea_orm_active_enums.rs

bacon *args:
    bacon clippy {{args}}

pre-commit:
    cargo fmt --check
    cargo clippy -- -D warnings
    cargo test --locked

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
    psql "$(tomlq -r .database.url < ${CONFIG_PATH})" {{args}}

redis ms *args:
    redis-cli -u "$(tomlq -r .redis.{{ms}} < ${CONFIG_PATH})" {{args}}

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
    mv entity/src entity/.src.bak
    mkdir entity/src
    if DATABASE_URL="$(tomlq -r .database.url < ${CONFIG_PATH})" sea generate entity -l -o entity/src --with-copy-enums; then rm -rf entity/.src.bak; else rm -rf entity/src; mv entity/.src.bak entity/src; exit 1; fi
    if [[ -f entity/src/sea_orm_active_enums.rs ]]; then sed -i -E 's/^(#\[derive\(.*DeriveActiveEnum.*)\)\]$/\1, poem_openapi::Enum, serde::Serialize, serde::Deserialize)]\n#[serde(rename_all = "SCREAMING_SNAKE_CASE")]\n#[oai(rename_all = "SCREAMING_SNAKE_CASE")]/' entity/src/sea_orm_active_enums.rs; fi

bacon *args:
    bacon clippy {{args}}

pre-commit:
    cargo fmt --check
    cargo clippy -- -D warnings
    cargo test --locked

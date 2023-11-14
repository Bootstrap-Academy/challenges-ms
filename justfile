alias r := run
alias c := check
alias t := test
alias p := psql
alias m := migrate
alias e := entity
alias b := bacon

_default:
    @just --list

# cargo run
run *args:
    cargo run --locked -p challenges {{args}}

# cargo clippy
check *args:
    cargo clippy {{args}}

# cargo test
test *args:
    cargo test --locked {{args}}

# connect to database
psql *args:
    psql "$DATABASE__URL" {{args}}

# connect to redis
redis ms *args:
    redis-cli -u "$REDIS__{{uppercase(ms)}}" {{args}}

# run migrations
migrate *args:
    sea migrate {{args}}

# generate entities
entity:
    mv entity/src entity/.src.bak
    mkdir entity/src
    if DATABASE_URL="$DATABASE__URL" sea generate entity -l -o entity/src --with-copy-enums; then rm -rf entity/.src.bak; else rm -rf entity/src; mv entity/.src.bak entity/src; exit 1; fi
    if [[ -f entity/src/sea_orm_active_enums.rs ]]; then sed -i -E 's/^(#\[derive\(.*DeriveActiveEnum.*)\)\]$/\1, poem_openapi::Enum, serde::Serialize, serde::Deserialize)]\n#[serde(rename_all = "SCREAMING_SNAKE_CASE")]\n#[oai(rename_all = "SCREAMING_SNAKE_CASE")]/' entity/src/sea_orm_active_enums.rs; fi
    cargo fmt -p entity

# bacon clippy
bacon *args:
    bacon clippy {{args}}

# run pre-commit checks
pre-commit:
    cargo fmt --check
    cargo clippy -- -D warnings
    cargo test --locked

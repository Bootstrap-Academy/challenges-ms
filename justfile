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
    psql "$(tomlq -r .database_url < .config.toml)" {{args}}

migrate *args:
    sea migrate {{args}}

entity:
    DATABASE_URL="$(tomlq -r .database_url < .config.toml)" sea generate entity -l -o entity/src --with-copy-enums
    sed -i -E 's/^(#\[derive\(.*DeriveActiveEnum.*)\)\]$/\1, poem_openapi::Enum)]/' entity/src/sea_orm_active_enums.rs

bacon *args:
    bacon clippy {{args}}

pre-commit:
    cargo fmt --check
    cargo clippy -- -D warnings
    cargo test --locked

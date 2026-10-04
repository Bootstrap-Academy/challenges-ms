//! History callbacks must be able to finish while every attempt transaction
//! waits for Skills. Keep the existing request transactions and their locks;
//! only the bounded, read-only history route uses a separate connection pool.
use std::{sync::Arc, time::Duration};

use lib::config::Config;
use poem::{http::Method, Endpoint, Middleware, Request, Response};
use poem_ext::db::{DbTransactionMiddleware, DbTransactionMwEndpoint};
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use uuid::Uuid;

pub struct RequestTransactions {
    primary: DatabaseConnection,
    history: DatabaseConnection,
}

impl RequestTransactions {
    pub async fn connect(primary: DatabaseConnection, config: &Config) -> anyhow::Result<Self> {
        let mut options = ConnectOptions::new(config.database.url.to_string());
        // One extra connection per API process is sufficient: history performs
        // local snapshot reads and never waits on Skills or attempt locks.
        options
            .min_connections(0)
            .max_connections(1)
            .connect_timeout(Duration::from_secs(config.database.connect_timeout));
        Ok(Self {
            primary,
            history: Database::connect(options).await?,
        })
    }
}

pub struct RequestTransactionEndpoint<E: Endpoint> {
    primary: DbTransactionMwEndpoint<Arc<E>>,
    history: DbTransactionMwEndpoint<Arc<E>>,
}

impl<E: Endpoint> Middleware<E> for RequestTransactions {
    type Output = RequestTransactionEndpoint<E>;

    fn transform(&self, inner: E) -> Self::Output {
        let inner = Arc::new(inner);
        RequestTransactionEndpoint {
            primary: DbTransactionMiddleware::new(self.primary.clone()).transform(inner.clone()),
            history: DbTransactionMiddleware::new(self.history.clone()).transform(inner),
        }
    }
}

impl<E: Endpoint> Endpoint for RequestTransactionEndpoint<E> {
    type Output = Response;

    async fn call(&self, req: Request) -> poem::Result<Response> {
        // Authentication and request validation still run in the same endpoint.
        // No other internal route, public read, or write gets this pool.
        let history = req.method() == Method::POST
            && req
                .uri()
                .path()
                .strip_prefix("/_internal/users/")
                .and_then(|path| path.strip_suffix("/learning-history"))
                .is_some_and(|subject| Uuid::parse_str(subject).is_ok());
        if history {
            self.history.call(req).await
        } else {
            self.primary.call(req).await
        }
    }
}

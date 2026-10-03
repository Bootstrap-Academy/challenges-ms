//! Explicit product-use admission; ordinary authentication remains separate.
use lib::{
    auth::{LearningPrincipal, User},
    services::Services,
};
use sea_orm::DatabaseTransaction;

pub async fn admit(
    db: &DatabaseTransaction,
    services: &Services,
    principal: LearningPrincipal,
) -> anyhow::Result<User> {
    super::benefits::lock_attempt(db, principal.user.id).await?;
    // Refresh after the local erasure wait; do not combine an old key with a
    // newly restored restriction/contact state. Backend returns one decision.
    let current = services
        .shop
        .learning_authority(&principal.digest)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Current limited learning admission unavailable"))?;
    anyhow::ensure!(
        current.id == principal.user.id,
        "Learning subject changed during admission"
    );
    Ok(current)
}

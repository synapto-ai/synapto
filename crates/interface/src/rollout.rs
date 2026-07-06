use crate::plugin::Plugin;
use crate::sync::watch;
use async_trait::async_trait;

#[async_trait]
pub trait RolloutController: Plugin + Send + Sync {
    async fn start(
        &self,
        rollout_tx: watch::Sender<crate::interaction::Timestamp>,
    ) -> Result<(), String>;
}

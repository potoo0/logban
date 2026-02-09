pub mod nftables;

use std::net::IpAddr;

use async_trait::async_trait;

#[async_trait]
pub trait Action: Send + Sync {
    async fn ban(&self, ip: IpAddr, rule_name: &str) -> anyhow::Result<()>;
    async fn unban(&self, ip: IpAddr, rule_name: &str) -> anyhow::Result<()>;
}

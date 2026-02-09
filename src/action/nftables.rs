use std::net::IpAddr;

use anyhow::Result;
use async_trait::async_trait;
use tokio::process::Command;
use tracing::info;

use crate::action::Action;

pub struct NftAction {
    table: String,
    set_name: String,
    dry_run: bool, // TODO implement dry_run functionality
}

impl NftAction {
    pub fn new(table: &str, set_name: &str, dry_run: bool) -> Self {
        Self { table: table.to_string(), set_name: set_name.to_string(), dry_run }
    }

    pub async fn init(&self) -> Result<()> {
        info!(dry_run = self.dry_run, "Initializing nftables table and set");
        // TODO
        // Command::new("nft").args(["add", "table", "inet", &self.table]).status().await?;
        // Command::new("nft")
        //     .args([
        //         "add",
        //         "set",
        //         "inet",
        //         &self.table,
        //         &self.set_name,
        //         "{ type ipv4_addr; flags timeout; }",
        //     ])
        //     .status()
        //     .await?;
        // Command::new("nft")
        //     .args([
        //         "add",
        //         "chain",
        //         "inet",
        //         &self.table,
        //         "input",
        //         "{ type filter hook input priority 0; policy accept; }",
        //     ])
        //     .status()
        //     .await?;
        // Command::new("nft")
        //     .args([
        //         "add",
        //         "rule",
        //         "inet",
        //         &self.table,
        //         "input",
        //         "ip",
        //         "saddr",
        //         &format!("@{}", self.set_name),
        //         "drop",
        //     ])
        //     .status()
        //     .await?;
        Ok(())
    }
}

#[async_trait]
impl Action for NftAction {
    async fn ban(&self, ip: IpAddr, _rule_name: &str) -> Result<()> {
        let status = Command::new("nft")
            .args(["add", "element", "inet", &self.table, &self.set_name, &format!("{{ {} }}", ip)])
            .status()
            .await?;
        if status.success() { Ok(()) } else { anyhow::bail!("nft command failed") }
    }

    async fn unban(&self, ip: IpAddr, _rule_name: &str) -> Result<()> {
        Command::new("nft")
            .args([
                "delete",
                "element",
                "inet",
                &self.table,
                &self.set_name,
                &format!("{{ {} }}", ip),
            ])
            .status()
            .await?;
        Ok(())
    }
}

//! Kill switch — leak protection built on Windows Firewall.
//!
//! Plain "block everything" rules do not work, because in Windows Firewall an
//! explicit block always beats an explicit allow. The reliable recipe is to
//! flip the **default outbound action** to block and then add targeted allow
//! rules for: this app, the VPN server, loopback, DHCP and (optionally) the
//! local subnet. Everything else is dropped the moment the tunnel dies.

#[cfg_attr(not(windows), allow(unused_imports))]
use crate::error::{CoreError, Result};
use crate::logging::LogBus;
use std::sync::Arc;

pub const RULE_PREFIX: &str = "NetworldVPN";

#[derive(Debug, Clone)]
pub struct KillSwitchPlan {
    /// Full path of the executable allowed to talk to the server.
    pub app_path: String,
    /// VPN server IP or host that must stay reachable.
    pub server_host: String,
    pub allow_lan: bool,
    pub block_ipv6: bool,
}

pub struct KillSwitch {
    logs: Arc<LogBus>,
    engaged: bool,
}

impl KillSwitch {
    pub fn new(logs: Arc<LogBus>) -> Self {
        Self {
            logs,
            engaged: false,
        }
    }

    pub fn is_engaged(&self) -> bool {
        self.engaged
    }

    pub async fn engage(&mut self, plan: &KillSwitchPlan) -> Result<()> {
        if !cfg!(windows) {
            self.logs
                .warn("killswitch", "Kill switch is Windows-only; skipping");
            return Ok(());
        }
        self.clear_rules().await;
        // From here rules may exist, so release() must clean up even if a later
        // step fails.
        self.engaged = true;

        // 1. Allow this app outbound.
        run(
            &[
                "advfirewall",
                "firewall",
                "add",
                "rule",
                &format!("name={RULE_PREFIX}-App"),
                "dir=out",
                "action=allow",
                "enable=yes",
                "profile=any",
                &format!("program={}", plan.app_path),
            ],
            &self.logs,
        )
        .await?;

        // 2. Allow the tunnel endpoint.
        if !plan.server_host.is_empty() {
            run(
                &[
                    "advfirewall",
                    "firewall",
                    "add",
                    "rule",
                    &format!("name={RULE_PREFIX}-Server"),
                    "dir=out",
                    "action=allow",
                    "enable=yes",
                    "profile=any",
                    &format!("remoteip={}", plan.server_host),
                ],
                &self.logs,
            )
            .await?;
        }

        // 3. Loopback + DHCP so the stack keeps working.
        run(
            &[
                "advfirewall",
                "firewall",
                "add",
                "rule",
                &format!("name={RULE_PREFIX}-Loopback"),
                "dir=out",
                "action=allow",
                "enable=yes",
                "profile=any",
                "remoteip=127.0.0.1,::1",
            ],
            &self.logs,
        )
        .await?;
        run(
            &[
                "advfirewall",
                "firewall",
                "add",
                "rule",
                &format!("name={RULE_PREFIX}-DHCP"),
                "dir=out",
                "action=allow",
                "enable=yes",
                "profile=any",
                "protocol=udp",
                "remoteport=67,68",
            ],
            &self.logs,
        )
        .await?;

        if plan.allow_lan {
            run(
                &[
                    "advfirewall",
                    "firewall",
                    "add",
                    "rule",
                    &format!("name={RULE_PREFIX}-LAN"),
                    "dir=out",
                    "action=allow",
                    "enable=yes",
                    "profile=any",
                    "remoteip=LocalSubnet",
                ],
                &self.logs,
            )
            .await?;
        }

        if plan.block_ipv6 {
            run(
                &[
                    "advfirewall",
                    "firewall",
                    "add",
                    "rule",
                    &format!("name={RULE_PREFIX}-BlockV6"),
                    "dir=out",
                    "action=block",
                    "enable=yes",
                    "profile=any",
                    "remoteip=::/0",
                ],
                &self.logs,
            )
            .await?;
        }

        // 4. Default-deny outbound.
        run(
            &[
                "advfirewall",
                "set",
                "allprofiles",
                "firewallpolicy",
                "blockinbound,blockoutbound",
            ],
            &self.logs,
        )
        .await?;

        self.engaged = true;
        self.logs.info(
            "killswitch",
            "Kill switch engaged — non-tunnel traffic blocked",
        );
        Ok(())
    }

    pub async fn release(&mut self) {
        // Nothing to undo unless we engaged it: skipping avoids a burst of
        // netsh calls (and firewall policy resets) on every plain disconnect.
        if !cfg!(windows) || !self.engaged {
            return;
        }
        let _ = run(
            &[
                "advfirewall",
                "set",
                "allprofiles",
                "firewallpolicy",
                "blockinbound,allowoutbound",
            ],
            &self.logs,
        )
        .await;
        self.clear_rules().await;
        if self.engaged {
            self.logs.info("killswitch", "Kill switch released");
        }
        self.engaged = false;
    }

    async fn clear_rules(&self) {
        for suffix in ["App", "Server", "Loopback", "DHCP", "LAN", "BlockV6"] {
            let _ = run(
                &[
                    "advfirewall",
                    "firewall",
                    "delete",
                    "rule",
                    &format!("name={RULE_PREFIX}-{suffix}"),
                ],
                &self.logs,
            )
            .await;
        }
    }
}

#[allow(unused_variables)]
async fn run(args: &[&str], logs: &Arc<LogBus>) -> Result<()> {
    #[cfg(windows)]
    {
        logs.trace("killswitch", format!("netsh {}", args.join(" ")));
        let out = tokio::process::Command::new("netsh")
            .args(args)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: never flash a console
            .output()
            .await
            .map_err(|e| CoreError::Other(format!("netsh failed: {e}")))?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stdout).trim().to_string();
            // Deleting a rule that does not exist is not an error for us.
            if !msg.contains("No rules match") {
                return Err(CoreError::Other(format!("netsh: {msg}")));
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

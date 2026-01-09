// Copyright (c) 2026 Hikaru (i@rua.moe)
// All rights reserved.
// This software is licensed under CC BY-NC 4.0
// Attribution required, Commercial use prohibited

use anyhow::{Context, Result};
use log::{debug, info, warn};
use tokio::process::Command;

use crate::config::NetworkInterface;

/// OpenWrt 路由管理器
pub struct OpenWrtManager {
    /// 当前活动接口
    current_interface: Option<String>,
    /// 路由规则标记（用于识别本程序创建的规则）
    #[allow(dead_code)]
    rule_marker: String,
}

impl OpenWrtManager {
    /// 创建新的 OpenWrt 管理器
    pub fn new() -> Self {
        Self {
            current_interface: None,
            rule_marker: "routes-monitor".to_string(),
        }
    }

    /// 将逻辑接口名转换为物理接口名
    /// pppoe-wan_cm -> wan_cm
    /// pppoe-wan_ct1 -> wan_ct1
    fn convert_to_physical_interface(logical_name: &str) -> String {
        logical_name.trim_start_matches("pppoe-").to_string()
    }

    /// 获取当前活动接口
    pub fn current_interface(&self) -> Option<&str> {
        self.current_interface.as_deref()
    }

    /// 切换到指定接口
    ///
    /// 重要：此方法只修改 UCI 配置并重载网络，不直接操作 ip route
    /// 这样可以避免中间状态导致的网络中断
    pub async fn switch_to_interface(
        &mut self,
        interface: &NetworkInterface,
        manage_uci_routes: bool,
        static_route_targets: Option<&[String]>,
    ) -> Result<()> {
        info!(
            "开始切换到接口: {} ({})",
            interface.name, interface.display_name
        );

        // 如果已经是当前接口，则跳过
        if let Some(current) = &self.current_interface {
            if current == &interface.name {
                info!("接口 {} 已经是当前活动接口，跳过切换", interface.name);
                return Ok(());
            }
        }

        // 使用 UCI 配置管理静态路由（持久化到 /etc/config/network）
        // 只修改 UCI 配置，让 OpenWrt 自己处理路由
        if manage_uci_routes {
            if let Some(targets) = static_route_targets {
                self.manage_static_routes(targets, &interface.name).await?;
            }
        }

        // 更新当前接口
        self.current_interface = Some(interface.name.clone());

        info!("接口切换成功: {}", interface.name);
        Ok(())
    }

    /// 获取当前所有策略路由规则
    async fn get_current_rules(&self) -> Result<Vec<String>> {
        let output = Command::new("ip")
            .args(&["rule", "show"])
            .output()
            .await
            .context("获取路由规则失败")?;

        let rules = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();

        Ok(rules)
    }

    /// 清除旧的路由规则
    /// 策略：
    /// 1. 清除默认路由（会被新接口的默认路由替代）
    /// 2. 清除所有优先级在 100-999 范围内的规则（本程序使用的范围）
    /// 3. 清除指定路由表中的所有路由
    async fn clear_old_routes(&self) -> Result<()> {
        info!("清除旧的路由规则...");

        // 1. 删除默认路由
        // 注意：可能返回错误（如果没有默认路由），我们忽略错误
        let _ = Command::new("ip")
            .args(&["route", "del", "default"])
            .output()
            .await;

        debug!("默认路由已清除");

        // 2. 获取现有规则，只删除我们创建的（优先级 100-999）
        if let Ok(rules) = self.get_current_rules().await {
            for rule in rules {
                // 检查是否是我们的优先级范围
                if let Some(priority) = self.extract_priority(&rule) {
                    if priority >= 100 && priority <= 999 {
                        debug!("删除路由规则: {}", rule);
                        let _ = Command::new("ip")
                            .args(&["rule", "del", "priority", &priority.to_string()])
                            .output()
                            .await;
                    }
                }
            }
        } else {
            // 如果无法获取规则列表，退回到批量删除
            warn!("无法获取规则列表，使用批量删除模式");
            for priority in 100..=999 {
                let _ = Command::new("ip")
                    .args(&["rule", "del", "priority", &priority.to_string()])
                    .output()
                    .await;
            }
        }

        debug!("策略路由规则已清除");

        Ok(())
    }

    /// 从规则字符串中提取优先级
    /// 例如: "100: from all lookup 100" -> Some(100)
    fn extract_priority(&self, rule: &str) -> Option<u32> {
        rule.split(':').next()?.trim().parse().ok()
    }

    /// 设置策略路由
    async fn setup_policy_routing(&self, interface: &NetworkInterface) -> Result<()> {
        info!("设置策略路由: {}", interface.name);

        // 如果配置了路由表 ID，设置策略路由
        if let Some(table_id) = interface.table_id {
            // 添加路由规则：从指定接口出去的流量使用指定路由表
            let output = Command::new("ip")
                .args(&[
                    "rule",
                    "add",
                    "oif",
                    &interface.name,
                    "table",
                    &table_id.to_string(),
                    "priority",
                    "100",
                ])
                .output()
                .await
                .context("执行 ip rule add 命令失败")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // 如果规则已存在，忽略错误
                if !stderr.contains("File exists") {
                    anyhow::bail!("添加路由规则失败: {}", stderr);
                }
            }

            debug!("策略路由规则已添加: table {}", table_id);

            // 在指定路由表中添加默认路由
            if let Some(gateway) = &interface.gateway {
                let output = Command::new("ip")
                    .args(&[
                        "route",
                        "add",
                        "default",
                        "via",
                        gateway,
                        "dev",
                        &interface.name,
                        "table",
                        &table_id.to_string(),
                    ])
                    .output()
                    .await
                    .context("执行 ip route add 命令失败")?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    // 如果路由已存在，忽略错误
                    if !stderr.contains("File exists") {
                        warn!("添加路由表默认路由失败: {}", stderr);
                    }
                }

                debug!("路由表 {} 默认路由已设置: {}", table_id, gateway);
            }
        }

        Ok(())
    }

    /// 设置默认网关
    async fn set_default_gateway(&self, interface: &NetworkInterface) -> Result<()> {
        info!("设置默认网关: {}", interface.name);

        // 如果没有配置网关，尝试自动获取
        let gateway = if let Some(gw) = &interface.gateway {
            gw.clone()
        } else {
            // 尝试从 DHCP 或网络配置获取网关
            self.get_interface_gateway(&interface.name).await?
        };

        // 添加默认路由
        let output = Command::new("ip")
            .args(&[
                "route",
                "add",
                "default",
                "via",
                &gateway,
                "dev",
                &interface.name,
            ])
            .output()
            .await
            .context("执行 ip route add default 命令失败")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("设置默认路由失败: {}", stderr);
        }

        info!("默认网关已设置: {} via {}", interface.name, gateway);

        Ok(())
    }

    /// 获取接口的网关地址
    async fn get_interface_gateway(&self, interface: &str) -> Result<String> {
        // 使用 UCI 命令获取接口配置（OpenWrt 特有）
        let output = Command::new("uci")
            .args(&["get", &format!("network.{}.gateway", interface)])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                let gateway = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !gateway.is_empty() {
                    return Ok(gateway);
                }
            }
            _ => {}
        }

        // 如果 UCI 失败，尝试从路由表获取
        let output = Command::new("ip")
            .args(&["route", "show", "dev", interface])
            .output()
            .await
            .context("获取接口路由失败")?;

        let routes = String::from_utf8_lossy(&output.stdout);
        for line in routes.lines() {
            if line.contains("default via") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(idx) = parts.iter().position(|&x| x == "via") {
                    if let Some(gateway) = parts.get(idx + 1) {
                        return Ok(gateway.to_string());
                    }
                }
            }
        }

        anyhow::bail!("无法获取接口 {} 的网关地址", interface)
    }

    /// 刷新路由缓存
    async fn flush_route_cache(&self) -> Result<()> {
        debug!("刷新路由缓存...");

        let output = Command::new("ip")
            .args(&["route", "flush", "cache"])
            .output()
            .await
            .context("执行 ip route flush cache 命令失败")?;

        if !output.status.success() {
            warn!("刷新路由缓存可能失败，但继续执行");
        }

        debug!("路由缓存已刷新");

        Ok(())
    }

    /// 验证接口切换是否成功
    /// 检查被监控的 UCI 静态路由是否已正确配置到目标接口
    pub async fn verify_switch(&self, interface: &NetworkInterface) -> Result<bool> {
        info!("验证接口切换: {}", interface.name);

        let physical_interface = Self::convert_to_physical_interface(&interface.name);

        // 检查 UCI 静态路由是否已配置到目标接口
        let routes = self.get_uci_static_routes().await?;

        if routes.is_empty() {
            info!("没有 UCI 静态路由需要验证");
            return Ok(true);
        }

        // 只验证由本程序管理的路由（route_ 前缀的命名路由）
        let managed_routes: Vec<_> = routes
            .iter()
            .filter(|(section, _, _)| section.starts_with("route_"))
            .collect();

        if managed_routes.is_empty() {
            info!("没有本程序管理的路由需要验证");
            return Ok(true);
        }

        let all_correct = managed_routes
            .iter()
            .all(|(_, _, iface)| iface == &physical_interface);

        if all_correct {
            info!(
                "接口切换验证成功: {} (物理接口: {})",
                interface.name, physical_interface
            );
        } else {
            warn!("接口切换验证失败: 部分路由未指向 {}", physical_interface);
            for (section, target, iface) in &managed_routes {
                if *iface != physical_interface {
                    warn!("  路由 {} ({}) 仍指向 {}", section, target, iface);
                }
            }
        }

        Ok(all_correct)
    }

    /// 备份当前路由配置
    #[allow(dead_code)]
    pub async fn backup_routes(&self) -> Result<String> {
        info!("备份当前路由配置...");

        let output = Command::new("ip")
            .args(&["route", "show"])
            .output()
            .await
            .context("获取路由表失败")?;

        let routes = String::from_utf8_lossy(&output.stdout).to_string();

        debug!("路由配置已备份，共 {} 字节", routes.len());

        Ok(routes)
    }

    /// 使用 ip route 命令管理静态路由（不持久化）
    /// 用于动态切换监控目标IP的路由，不修改UCI配置
    async fn manage_ip_static_routes(&self, targets: &[String], interface: &str) -> Result<()> {
        info!("更新静态IP路由到接口: {}", interface);

        for target in targets {
            // 删除旧路由（如果存在）
            let _ = Command::new("ip")
                .args(&["route", "del", target])
                .output()
                .await;

            // 添加新路由
            let output = Command::new("ip")
                .args(&["route", "add", target, "dev", interface])
                .output()
                .await
                .context(format!("添加路由 {} 失败", target))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.contains("File exists") {
                    warn!("添加路由 {} 到 {} 失败: {}", target, interface, stderr);
                }
            } else {
                info!("已添加路由: {} -> {}", target, interface);
            }
        }

        Ok(())
    }

    /// 使用 UCI 持久化配置（OpenWrt 特有）
    #[allow(dead_code)]
    pub async fn persist_config(&self, interface: &NetworkInterface) -> Result<()> {
        info!("持久化接口配置: {}", interface.name);

        // 设置网络接口优先级
        let _ = Command::new("uci")
            .args(&[
                "set",
                &format!("network.{}.metric", interface.name),
                &interface.priority.to_string(),
            ])
            .output()
            .await;

        // 提交配置
        let output = Command::new("uci")
            .args(&["commit", "network"])
            .output()
            .await
            .context("提交 UCI 配置失败")?;

        if !output.status.success() {
            warn!("UCI 配置提交可能失败");
        }

        info!("接口配置已持久化");

        Ok(())
    }

    /// 重启网络服务（慎用）
    #[allow(dead_code)]
    pub async fn restart_network(&self) -> Result<()> {
        warn!("重启网络服务...");

        let output = Command::new("/etc/init.d/network")
            .arg("restart")
            .output()
            .await
            .context("重启网络服务失败")?;

        if !output.status.success() {
            anyhow::bail!("网络服务重启失败");
        }

        info!("网络服务已重启");

        Ok(())
    }

    /// 更新 UCI 配置中的静态路由接口
    /// 策略：替换接口而非删除配置，保留目标 IP
    /// 如果路由不存在，则创建新的路由
    async fn update_uci_routes(
        &self,
        old_interface: Option<&str>,
        new_interface: &str,
    ) -> Result<()> {
        info!("更新 UCI 静态路由配置...");

        // 1. 获取所有静态路由配置
        let routes = self.get_uci_static_routes().await?;

        if routes.is_empty() {
            debug!("没有找到 UCI 静态路由配置");
            return Ok(());
        }

        info!("找到 {} 条 UCI 静态路由", routes.len());

        // 2. 更新每条路由的接口
        for (section, target, interface) in routes {
            // 如果指定了旧接口，只更新匹配的路由
            // 如果没有指定，更新所有静态路由
            let should_update = if let Some(old_iface) = old_interface {
                interface == old_iface
            } else {
                true
            };

            if should_update {
                info!(
                    "更新路由 {} (目标: {}) 从接口 {} 到 {}",
                    section, target, interface, new_interface
                );

                // 使用 uci set 命令替换接口
                let output = Command::new("uci")
                    .args(&[
                        "set",
                        &format!("network.{}.interface={}", section, new_interface),
                    ])
                    .output()
                    .await
                    .context("执行 uci set 命令失败")?;

                if !output.status.success() {
                    warn!(
                        "更新路由 {} 失败: {}",
                        section,
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
        }

        // 3. 提交并应用更改
        self.commit_uci_changes().await?;

        info!("UCI 静态路由更新完成");
        Ok(())
    }

    /// 获取 UCI 配置中的所有静态路由
    /// 返回: Vec<(section_name, target, interface)>
    async fn get_uci_static_routes(&self) -> Result<Vec<(String, String, String)>> {
        let output = Command::new("uci")
            .args(&["show", "network"])
            .output()
            .await
            .context("执行 uci show 命令失败")?;

        if !output.status.success() {
            anyhow::bail!("获取 UCI 配置失败");
        }

        let config = String::from_utf8_lossy(&output.stdout);
        let mut routes = Vec::new();

        // 解析 UCI 配置，查找 route 类型的配置段
        // 支持两种格式:
        // 1. 命名路由: network.route_name=route
        // 2. 数组路由: network.@route[0]=route
        let mut temp_routes: std::collections::HashMap<String, (Option<String>, Option<String>)> =
            std::collections::HashMap::new();

        for line in config.lines() {
            // 匹配配置段: network.route_name=route 或 network.@route[0]=route
            if line.contains("=route") {
                if let Some(section) = line.split('=').next() {
                    let section_name = section.trim_start_matches("network.");
                    temp_routes.insert(section_name.to_string(), (None, None));
                }
            }
            // 匹配 target 和 interface
            else {
                for (section, (target, interface)) in temp_routes.iter_mut() {
                    let target_key = format!("network.{}.target", section);
                    let interface_key = format!("network.{}.interface", section);

                    if line.starts_with(&target_key) {
                        if let Some(value) = Self::extract_uci_value(line) {
                            *target = Some(value);
                        }
                    } else if line.starts_with(&interface_key) {
                        if let Some(value) = Self::extract_uci_value(line) {
                            *interface = Some(value);
                        }
                    }
                }
            }
        }

        // 收集完整的路由信息
        for (section, (target, interface)) in temp_routes {
            if let (Some(t), Some(i)) = (target, interface) {
                routes.push((section, t, i));
            }
        }

        debug!("找到 {} 条 UCI 静态路由", routes.len());

        Ok(routes)
    }

    /// 从 UCI 配置行中提取值
    /// 例如: "network.route_wan.interface='wan_cm'" -> Some("wan_cm")
    fn extract_uci_value(line: &str) -> Option<String> {
        if let Some(value_part) = line.split('=').nth(1) {
            let value = value_part.trim().trim_matches('\'').trim_matches('"');
            return Some(value.to_string());
        }
        None
    }

    /// 提交 UCI 更改并使网络配置生效
    async fn commit_uci_changes(&self) -> Result<()> {
        info!("提交 UCI 配置更改并使网络生效...");

        // 1. 提交 network 配置
        let output = Command::new("uci")
            .args(&["commit", "network"])
            .output()
            .await
            .context("提交 UCI 配置失败")?;

        if !output.status.success() {
            anyhow::bail!(
                "UCI commit 失败: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        debug!("UCI 配置已提交");

        // 2. 重新加载网络配置（使用 reload 而不是 restart，避免中断连接）
        let output = Command::new("/etc/init.d/network")
            .arg("reload")
            .output()
            .await
            .context("重载网络配置失败")?;

        if !output.status.success() {
            warn!(
                "网络配置重载可能失败: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        } else {
            info!("网络配置已重载，静态路由已生效");
        }

        // 3. 等待网络配置应用（给系统一些时间）
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        Ok(())
    }

    /// 为指定目标 IP 列表创建或更新 UCI 静态路由
    /// 用于管理配置文件中指定的目标 IP
    /// 只更新被监控的目标，使用物理接口名
    pub async fn manage_static_routes(&self, targets: &[String], interface: &str) -> Result<()> {
        // 转换为物理接口名
        let physical_interface = Self::convert_to_physical_interface(interface);
        info!(
            "管理 {} 个目标 IP 的静态路由，逻辑接口: {} -> 物理接口: {}",
            targets.len(),
            interface,
            physical_interface
        );

        // 获取现有路由
        let existing_routes = self.get_uci_static_routes().await?;

        for target in targets {
            // 查找是否已存在该目标的路由（支持带/32或不带/32）
            let target_base = target.trim_end_matches("/32");
            let existing_route = existing_routes.iter().find(|(_, t, _)| {
                let t_base = t.trim_end_matches("/32");
                t_base == target_base
            });

            if let Some((section, _, old_interface)) = existing_route {
                // 路由已存在，检查是否需要更新接口
                if old_interface != &physical_interface {
                    info!(
                        "更新被监控路由: {} ({} -> {})",
                        target, old_interface, physical_interface
                    );
                    let cmd = format!("network.{}.interface={}", section, physical_interface);
                    let output = Command::new("uci")
                        .args(&["set", &cmd])
                        .output()
                        .await
                        .context("更新 UCI 路由失败")?;

                    if !output.status.success() {
                        warn!(
                            "更新路由接口失败: {}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }
                } else {
                    debug!(
                        "被监控路由 {} 接口已正确设置为 {}",
                        target, physical_interface
                    );
                }
            } else {
                // 路由不存在，创建新路由
                info!("创建新静态路由: {} via {}", target, physical_interface);
                self.create_uci_route(target, &physical_interface).await?;
            }
        }

        // 提交更改
        self.commit_uci_changes().await?;

        Ok(())
    }

    /// 创建新的 UCI 静态路由
    async fn create_uci_route(&self, target: &str, interface: &str) -> Result<()> {
        // 生成路由名称（使用 IP 作为标识）
        let route_name = format!(
            "route_{}",
            target.replace('/', "_").replace('.', "_").replace(':', "_")
        );

        debug!("创建 UCI 路由: {} -> {}", route_name, target);

        // 创建路由配置段
        let commands = vec![
            format!("network.{}=route", route_name),
            format!("network.{}.interface={}", route_name, interface),
            format!("network.{}.target={}", route_name, target),
        ];

        for cmd in commands {
            let output = Command::new("uci")
                .args(&["set", &cmd])
                .output()
                .await
                .context("执行 uci set 命令失败")?;

            if !output.status.success() {
                warn!(
                    "UCI set 失败 ({}): {}",
                    cmd,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        info!("静态路由 {} 创建成功", route_name);
        Ok(())
    }
}

impl Default for OpenWrtManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openwrt_manager_creation() {
        let manager = OpenWrtManager::new();
        assert!(manager.current_interface().is_none());
    }
}

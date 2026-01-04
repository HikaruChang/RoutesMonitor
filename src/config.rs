use anyhow::{Context, Result};
// Copyright (c) 2026 Hikaru (i@rua.moe)
// All rights reserved.
// This software is licensed under CC BY-NC 4.0
// Attribution required, Commercial use prohibited

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// 主配置结构体
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    /// 全局设置
    pub global: GlobalConfig,
    /// 网络接口列表
    pub interfaces: Vec<NetworkInterface>,
    /// 要监控的目标 IP 列表
    pub targets: Vec<TargetIP>,
}

/// 全局配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlobalConfig {
    /// 检查间隔（秒）
    pub check_interval: u64,
    /// 超时时间（秒）
    pub timeout: u64,
    /// 并发测试数量
    pub concurrent_tests: usize,
    /// 连续失败多少次才切换接口
    pub failure_threshold: u32,
    /// 日志级别 (trace, debug, info, warn, error)
    pub log_level: String,
    /// 是否启用自动切换
    pub auto_switch: bool,
}

/// 网络接口配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NetworkInterface {
    /// 接口名称 (例如: eth0, wlan0)
    pub name: String,
    /// 接口显示名称
    pub display_name: String,
    /// 优先级（数字越小优先级越高）
    pub priority: u32,
    /// 是否启用此接口
    pub enabled: bool,
    /// 路由表 ID（用于策略路由）
    pub table_id: Option<u32>,
    /// 网关地址
    pub gateway: Option<String>,
}

/// 目标 IP 配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TargetIP {
    /// IP 地址或域名
    pub address: String,
    /// 描述
    pub description: String,
    /// 测试 URL（用于速度测试，可选）
    pub test_url: Option<String>,
    /// 权重（影响速度评分）
    pub weight: f64,
}

impl Config {
    /// 从文件加载配置
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("无法读取配置文件: {:?}", path.as_ref()))?;

        let config: Config = toml::from_str(&content).with_context(|| "配置文件解析失败")?;

        config.validate()?;

        Ok(config)
    }

    /// 验证配置有效性
    pub fn validate(&self) -> Result<()> {
        // 验证至少有一个启用的接口
        if !self.interfaces.iter().any(|i| i.enabled) {
            anyhow::bail!("至少需要启用一个网络接口");
        }

        // 验证至少有一个目标 IP
        if self.targets.is_empty() {
            anyhow::bail!("至少需要配置一个目标 IP");
        }

        // 验证全局配置
        if self.global.check_interval == 0 {
            anyhow::bail!("检查间隔不能为 0");
        }

        if self.global.timeout == 0 {
            anyhow::bail!("超时时间不能为 0");
        }

        if self.global.concurrent_tests == 0 {
            anyhow::bail!("并发测试数量不能为 0");
        }

        // 验证接口名称唯一性
        let mut names = std::collections::HashSet::new();
        for interface in &self.interfaces {
            if !names.insert(interface.name.clone()) {
                anyhow::bail!("接口名称重复: {}", interface.name);
            }
        }

        Ok(())
    }

    /// 获取启用的接口列表
    pub fn enabled_interfaces(&self) -> Vec<&NetworkInterface> {
        self.interfaces.iter().filter(|i| i.enabled).collect()
    }

    /// 根据优先级排序的接口列表
    pub fn sorted_interfaces(&self) -> Vec<&NetworkInterface> {
        let mut interfaces = self.enabled_interfaces();
        interfaces.sort_by_key(|i| i.priority);
        interfaces
    }
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            check_interval: 60,
            timeout: 5,
            concurrent_tests: 4,
            failure_threshold: 3,
            log_level: "info".to_string(),
            auto_switch: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation() {
        let config = Config {
            global: GlobalConfig::default(),
            interfaces: vec![NetworkInterface {
                name: "eth0".to_string(),
                display_name: "以太网".to_string(),
                priority: 1,
                enabled: true,
                table_id: Some(100),
                gateway: Some("192.168.1.1".to_string()),
            }],
            targets: vec![TargetIP {
                address: "8.8.8.8".to_string(),
                description: "Google DNS".to_string(),
                test_url: None,
                weight: 1.0,
            }],
        };

        assert!(config.validate().is_ok());
    }
}

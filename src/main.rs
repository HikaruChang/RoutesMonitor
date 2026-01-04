// Copyright (c) 2026 Hikaru (i@rua.moe)
// All rights reserved.
// This software is licensed under CC BY-NC 4.0
// Attribution required, Commercial use prohibited

mod config;
mod network;
mod openwrt;

use anyhow::{Context, Result};
use log::{error, info, warn};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

use config::Config;
use network::{InterfaceScore, NetworkTester};
use openwrt::OpenWrtManager;

/// 应用程序状态
struct AppState {
    /// 配置
    config: Config,
    /// 网络测试器
    tester: NetworkTester,
    /// OpenWrt 管理器
    manager: Arc<RwLock<OpenWrtManager>>,
    /// 连续失败计数
    failure_count: Arc<RwLock<std::collections::HashMap<String, u32>>>,
}

impl AppState {
    fn new(config: Config) -> Self {
        let tester = NetworkTester::new(config.global.timeout, config.global.concurrent_tests);

        Self {
            config,
            tester,
            manager: Arc::new(RwLock::new(OpenWrtManager::new())),
            failure_count: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    info!("========================================");
    info!("  OpenWrt 路由监控工具");
    info!("  Copyright (c) 2026 Hikaru (i@rua.moe)");
    info!("  All rights reserved.");
    info!("========================================");

    // 加载配置文件
    let config_path = get_config_path()?;
    info!("加载配置文件: {:?}", config_path);

    let config = Config::from_file(&config_path).context("加载配置文件失败")?;

    info!("配置加载成功:");
    info!("  - 监控间隔: {} 秒", config.global.check_interval);
    info!("  - 超时时间: {} 秒", config.global.timeout);
    info!("  - 并发测试: {} 个", config.global.concurrent_tests);
    info!("  - 失败阈值: {} 次", config.global.failure_threshold);
    info!(
        "  - 自动切换: {}",
        if config.global.auto_switch {
            "启用"
        } else {
            "禁用"
        }
    );
    info!("  - 接口数量: {}", config.interfaces.len());
    info!("  - 目标数量: {}", config.targets.len());

    // 创建应用状态
    let state = Arc::new(AppState::new(config));

    // 显示接口信息
    print_interfaces(&state.config);

    // 显示目标信息
    print_targets(&state.config);

    info!("========================================");
    info!("开始监控循环...");
    info!("========================================");

    // 主监控循环
    run_monitor_loop(state).await?;

    Ok(())
}

/// 获取配置文件路径
fn get_config_path() -> Result<PathBuf> {
    // 优先使用环境变量指定的路径
    if let Ok(path) = std::env::var("ROUTES_MONITOR_CONFIG") {
        return Ok(PathBuf::from(path));
    }

    // 检查当前目录
    let current_dir = PathBuf::from("config.toml");
    if current_dir.exists() {
        return Ok(current_dir);
    }

    // 检查 /etc 目录
    let etc_path = PathBuf::from("/etc/routes-monitor/config.toml");
    if etc_path.exists() {
        return Ok(etc_path);
    }

    // 使用默认路径
    Ok(current_dir)
}

/// 打印接口信息
fn print_interfaces(config: &Config) {
    info!("配置的网络接口:");
    for interface in &config.interfaces {
        info!(
            "  - {} ({}) - 优先级: {}, 状态: {}, 网关: {}",
            interface.name,
            interface.display_name,
            interface.priority,
            if interface.enabled {
                "启用"
            } else {
                "禁用"
            },
            interface.gateway.as_deref().unwrap_or("自动")
        );
    }
}

/// 打印目标信息
fn print_targets(config: &Config) {
    info!("监控目标:");
    for target in &config.targets {
        info!(
            "  - {} ({}) - 权重: {}",
            target.address, target.description, target.weight
        );
    }
}

/// 运行监控循环
async fn run_monitor_loop(state: Arc<AppState>) -> Result<()> {
    let mut iteration = 0u64;

    loop {
        iteration += 1;
        info!("");
        info!(
            "==================== 第 {} 次检查 ====================",
            iteration
        );

        // 执行一次检查
        if let Err(e) = run_single_check(&state).await {
            error!("检查过程出错: {}", e);
            error!("将在 {} 秒后重试...", state.config.global.check_interval);
        }

        // 等待下一次检查
        info!(
            "等待 {} 秒后进行下一次检查...",
            state.config.global.check_interval
        );
        sleep(Duration::from_secs(state.config.global.check_interval)).await;
    }
}

/// 执行单次检查
async fn run_single_check(state: &AppState) -> Result<()> {
    let start_time = std::time::Instant::now();

    // 获取启用的接口
    let interfaces = state.config.sorted_interfaces();

    if interfaces.is_empty() {
        warn!("没有启用的接口，跳过检查");
        return Ok(());
    }

    info!("开始测试 {} 个接口...", interfaces.len());

    // 测试所有接口
    let results = state
        .tester
        .test_all_interfaces(&interfaces, &state.config.targets)
        .await;

    // 计算评分
    let scores = state.tester.calculate_scores(&results);

    // 显示结果
    print_test_results(&scores);

    // 获取最佳接口
    if let Some(best) = state.tester.get_best_interface(&scores) {
        info!("最佳接口: {} (评分: {:.2})", best.interface, best.score);

        // 检查是否需要切换
        let should_switch = should_switch_interface(state, best).await?;

        if should_switch && state.config.global.auto_switch {
            // 查找接口配置
            if let Some(interface_config) = state
                .config
                .interfaces
                .iter()
                .find(|i| i.name == best.interface)
            {
                // 执行切换
                info!("准备切换到接口: {}", best.interface);

                let mut manager = state.manager.write().await;

                match manager
                    .switch_to_interface(
                        interface_config,
                        false, // 不再管理UCI路由
                        None,
                    )
                    .await
                {
                    Ok(_) => {
                        info!("接口切换成功!");

                        // 验证切换
                        if let Ok(verified) = manager.verify_switch(interface_config).await {
                            if verified {
                                info!("接口切换验证通过");

                                // 重置失败计数
                                let mut failures = state.failure_count.write().await;
                                failures.clear();
                            } else {
                                warn!("接口切换验证失败，可能需要手动检查");
                            }
                        }
                    }
                    Err(e) => {
                        error!("接口切换失败: {}", e);
                    }
                }
            }
        } else if !state.config.global.auto_switch {
            info!("自动切换已禁用，跳过接口切换");
        } else {
            info!("当前接口表现良好，无需切换");
        }
    } else {
        warn!("没有可用的接口!");
    }

    let elapsed = start_time.elapsed();
    info!("本次检查耗时: {:.2} 秒", elapsed.as_secs_f64());

    Ok(())
}

/// 判断是否应该切换接口
async fn should_switch_interface(state: &AppState, best: &InterfaceScore) -> Result<bool> {
    let manager = state.manager.read().await;

    // 如果没有当前接口，应该切换
    let current = match manager.current_interface() {
        Some(iface) => iface,
        None => {
            info!("尚未设置活动接口，需要切换");
            return Ok(true);
        }
    };

    // 如果最佳接口就是当前接口，不需要切换
    if current == best.interface {
        info!("当前接口 {} 已是最佳接口", current);

        // 重置失败计数
        let mut failures = state.failure_count.write().await;
        failures.insert(current.to_string(), 0);

        return Ok(false);
    }

    // 检查当前接口的失败次数
    let mut failures = state.failure_count.write().await;
    let current_failures = failures.entry(current.to_string()).or_insert(0);
    *current_failures += 1;

    info!(
        "当前接口 {} 已连续 {} 次非最佳 (阈值: {})",
        current, current_failures, state.config.global.failure_threshold
    );

    // 如果失败次数超过阈值，应该切换
    if *current_failures >= state.config.global.failure_threshold {
        info!("达到切换阈值，准备切换接口");
        return Ok(true);
    }

    Ok(false)
}

/// 打印测试结果
fn print_test_results(scores: &[InterfaceScore]) {
    info!("");
    info!("测试结果汇总:");
    info!(
        "{:<15} {:<8} {:<12} {:<12} {:<12} {:<8}",
        "接口", "可达", "延迟(ms)", "丢包率(%)", "速度(KB/s)", "评分"
    );
    info!("{}", "-".repeat(75));

    for score in scores {
        info!(
            "{:<15} {:<8} {:<12.2} {:<12.1} {:<12.2} {:<8.2}",
            score.interface,
            score.reachable_count,
            score.avg_latency_ms,
            score.avg_packet_loss * 100.0,
            score.avg_speed,
            score.score
        );
    }
    info!("");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_path_priority() {
        // 测试配置文件路径优先级
        let path = get_config_path();
        assert!(path.is_ok());
    }
}

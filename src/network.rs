// Copyright (c) 2026 Hikaru (i@rua.moe)
// All rights reserved.
// This software is licensed under CC BY-NC 4.0
// Attribution required, Commercial use prohibited

use anyhow::{Context, Result};
use futures::future::join_all;
use log::{debug, info, warn};
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::time::timeout;

use crate::config::{NetworkInterface, TargetIP};

/// 网络测试结果
#[derive(Debug, Clone)]
pub struct TestResult {
    /// 接口名称
    pub interface: String,
    /// 目标地址
    #[allow(dead_code)]
    pub target: String,
    /// 是否可达
    pub reachable: bool,
    /// 延迟（毫秒）
    pub latency_ms: Option<f64>,
    /// 丢包率（0.0-1.0）
    pub packet_loss: Option<f64>,
    /// 下载速度（KB/s）
    pub download_speed: Option<f64>,
    /// 测试时间
    #[allow(dead_code)]
    pub tested_at: chrono::DateTime<chrono::Local>,
}

/// 接口综合评分
#[derive(Debug, Clone)]
pub struct InterfaceScore {
    /// 接口名称
    pub interface: String,
    /// 可达目标数量
    pub reachable_count: usize,
    /// 平均延迟
    pub avg_latency_ms: f64,
    /// 平均丢包率
    pub avg_packet_loss: f64,
    /// 平均速度
    pub avg_speed: f64,
    /// 综合评分（越高越好）
    pub score: f64,
}

/// 网络测试器
pub struct NetworkTester {
    timeout_duration: Duration,
    concurrent_tests: usize,
}

impl NetworkTester {
    /// 创建新的网络测试器
    pub fn new(timeout_secs: u64, concurrent_tests: usize) -> Self {
        Self {
            timeout_duration: Duration::from_secs(timeout_secs),
            concurrent_tests,
        }
    }

    /// 测试单个接口到单个目标的连接
    pub async fn test_single(&self, interface: &NetworkInterface, target: &TargetIP) -> TestResult {
        debug!(
            "测试接口 {} 到目标 {} ({})",
            interface.name, target.address, target.description
        );

        // 移除 CIDR 后缀（如 /32）以进行 ping 测试
        let ping_target = target.address.split('/').next().unwrap_or(&target.address);

        // 进行 ping 测试（4次）并解析结果
        let (reachable, latency_ms, packet_loss) = self
            .ping_test_with_stats(&interface.name, ping_target, 4)
            .await;

        // 如果配置了测试 URL，进行速度测试
        let download_speed = if reachable && target.test_url.is_some() {
            self.speed_test(&interface.name, target.test_url.as_ref().unwrap())
                .await
                .ok()
        } else {
            None
        };

        TestResult {
            interface: interface.name.clone(),
            target: target.address.clone(),
            reachable,
            latency_ms,
            packet_loss,
            download_speed,
            tested_at: chrono::Local::now(),
        }
    }

    /// 测试单个接口到所有目标
    pub async fn test_interface(
        &self,
        interface: &NetworkInterface,
        targets: &[TargetIP],
    ) -> Vec<TestResult> {
        info!(
            "开始测试接口: {} ({})",
            interface.name, interface.display_name
        );

        let mut results = Vec::new();

        // 分批并发测试
        for chunk in targets.chunks(self.concurrent_tests) {
            let mut tasks = Vec::new();

            for target in chunk {
                let task = self.test_single(interface, target);
                tasks.push(task);
            }

            // 使用 tokio 的 join_all
            let chunk_results = join_all(tasks).await;
            results.extend(chunk_results);
        }

        results
    }

    /// 测试所有接口（并发测试所有接口）
    pub async fn test_all_interfaces(
        &self,
        interfaces: &[&NetworkInterface],
        targets: &[TargetIP],
    ) -> Vec<TestResult> {
        info!(
            "开始并发测试所有接口，共 {} 个接口，{} 个目标",
            interfaces.len(),
            targets.len()
        );

        // 为每个接口创建测试任务
        let mut tasks = Vec::new();
        for interface in interfaces {
            let task = self.test_interface(interface, targets);
            tasks.push(task);
        }

        // 并发执行所有接口的测试
        let results_vec = join_all(tasks).await;

        // 合并所有结果
        let mut all_results = Vec::new();
        for results in results_vec {
            all_results.extend(results);
        }

        all_results
    }

    /// 计算接口评分
    pub fn calculate_scores(&self, results: &[TestResult]) -> Vec<InterfaceScore> {
        // 按接口分组
        let mut interface_results: std::collections::HashMap<String, Vec<&TestResult>> =
            std::collections::HashMap::new();

        for result in results {
            interface_results
                .entry(result.interface.clone())
                .or_insert_with(Vec::new)
                .push(result);
        }

        // 计算每个接口的评分
        let mut scores = Vec::new();

        for (interface, iface_results) in interface_results {
            let reachable_results: Vec<_> = iface_results.iter().filter(|r| r.reachable).collect();

            let reachable_count = reachable_results.len();

            if reachable_count == 0 {
                // 完全不可达的接口得分为 0
                scores.push(InterfaceScore {
                    interface,
                    reachable_count: 0,
                    avg_latency_ms: f64::INFINITY,
                    avg_packet_loss: 1.0,
                    avg_speed: 0.0,
                    score: 0.0,
                });
                continue;
            }

            // 计算平均延迟
            let latencies: Vec<f64> = reachable_results
                .iter()
                .filter_map(|r| r.latency_ms)
                .collect();

            let avg_latency_ms = if !latencies.is_empty() {
                latencies.iter().sum::<f64>() / latencies.len() as f64
            } else {
                100.0 // 默认延迟
            };

            // 计算平均丢包率
            let packet_losses: Vec<f64> = reachable_results
                .iter()
                .filter_map(|r| r.packet_loss)
                .collect();

            let avg_packet_loss = if !packet_losses.is_empty() {
                packet_losses.iter().sum::<f64>() / packet_losses.len() as f64
            } else {
                0.0
            };

            // 计算平均速度
            let speeds: Vec<f64> = reachable_results
                .iter()
                .filter_map(|r| r.download_speed)
                .collect();

            let avg_speed = if !speeds.is_empty() {
                speeds.iter().sum::<f64>() / speeds.len() as f64
            } else {
                0.0
            };

            // 综合评分计算
            // 公式: score = (reachable_ratio * 30) + (latency_score * 25) + (packet_loss_score * 25) + (speed_score * 20)
            let reachable_ratio = reachable_count as f64 / iface_results.len() as f64;

            // 延迟评分：延迟越低分数越高（使用倒数归一化）
            let latency_score = if avg_latency_ms > 0.0 {
                (1000.0 / avg_latency_ms).min(100.0)
            } else {
                100.0
            };

            // 丢包率评分：丢包率越低分数越高
            let packet_loss_score = (1.0 - avg_packet_loss) * 100.0;

            // 速度评分：速度越高分数越高（以 1MB/s 为满分基准）
            let speed_score = (avg_speed / 1024.0 * 100.0).min(100.0);

            // 评分权重：优先速度(40%)、其次丢包率(20%)、最后延迟(10%)，基础可达性(30%)
            let score = (reachable_ratio * 30.0)
                + (speed_score * 0.40)
                + (packet_loss_score * 0.20)
                + (latency_score * 0.10);

            scores.push(InterfaceScore {
                interface,
                reachable_count,
                avg_latency_ms,
                avg_packet_loss,
                avg_speed,
                score,
            });
        }

        // 按评分降序排序
        scores.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        scores
    }

    /// 使用 ping 测试连接性（简单版本，向后兼容）
    #[allow(dead_code)]
    async fn ping_test(&self, interface: &str, target: &str) -> bool {
        let (reachable, _, _) = self.ping_test_with_stats(interface, target, 1).await;
        reachable
    }

    /// 使用 ping 测试连接性并返回统计信息
    /// 返回: (是否可达, 平均延迟ms, 丢包率0.0-1.0)
    async fn ping_test_with_stats(
        &self,
        interface: &str,
        target: &str,
        count: u32,
    ) -> (bool, Option<f64>, Option<f64>) {
        // 在 OpenWrt 上使用 ping 命令测试连接
        // -I 指定接口，-c 指定次数，-W 指定超时
        let result = timeout(
            self.timeout_duration * count,
            Command::new("ping")
                .arg("-I")
                .arg(interface)
                .arg("-c")
                .arg(count.to_string())
                .arg("-W")
                .arg(format!("{}", self.timeout_duration.as_secs()))
                .arg(target)
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);

                // 解析 ping 输出
                // 格式示例: "4 packets transmitted, 3 received, 25% packet loss, time 3005ms"
                // "rtt min/avg/max/mdev = 10.123/15.456/20.789/3.210 ms"

                let mut packet_loss = None;
                let mut avg_latency = None;

                for line in stdout.lines() {
                    // 解析丢包率
                    if line.contains("packet loss") {
                        if let Some(loss_str) = line.split('%').next() {
                            if let Some(num_str) = loss_str.split_whitespace().last() {
                                if let Ok(loss) = num_str.parse::<f64>() {
                                    packet_loss = Some(loss / 100.0);
                                }
                            }
                        }
                    }

                    // 解析平均延迟
                    if line.contains("rtt") || line.contains("round-trip") {
                        // 格式: "rtt min/avg/max/mdev = 10.123/15.456/20.789/3.210 ms"
                        if let Some(stats_part) = line.split('=').nth(1) {
                            let parts: Vec<&str> = stats_part.trim().split('/').collect();
                            if parts.len() >= 2 {
                                if let Ok(avg) = parts[1].trim().parse::<f64>() {
                                    avg_latency = Some(avg);
                                }
                            }
                        }
                    }
                }

                let reachable = output.status.success() && packet_loss.unwrap_or(1.0) < 1.0;

                if reachable {
                    debug!(
                        "Ping 成功: {} -> {} (延迟: {:.2}ms, 丢包: {:.1}%)",
                        interface,
                        target,
                        avg_latency.unwrap_or(0.0),
                        packet_loss.unwrap_or(0.0) * 100.0
                    );
                } else {
                    debug!("Ping 失败: {} -> {}", interface, target);
                }

                (reachable, avg_latency, packet_loss)
            }
            Ok(Err(e)) => {
                warn!("执行 ping 命令失败: {}", e);
                (false, None, Some(1.0))
            }
            Err(_) => {
                warn!("Ping 超时: {} -> {}", interface, target);
                (false, None, Some(1.0))
            }
        }
    }

    /// 速度测试
    async fn speed_test(&self, interface: &str, test_url: &str) -> Result<f64> {
        let _start = Instant::now();

        // 使用 curl 通过指定接口下载测试文件
        let result = timeout(
            self.timeout_duration * 2, // 速度测试给更多时间
            Command::new("curl")
                .arg("--interface")
                .arg(interface)
                .arg("-s")
                .arg("-o")
                .arg("/dev/null")
                .arg("-w")
                .arg("%{speed_download}")
                .arg(test_url)
                .output(),
        )
        .await
        .context("速度测试超时")?
        .context("执行 curl 命令失败")?;

        if !result.status.success() {
            anyhow::bail!("curl 命令执行失败");
        }

        let speed_bytes = String::from_utf8_lossy(&result.stdout)
            .trim()
            .parse::<f64>()
            .context("解析速度数据失败")?;

        // 转换为 KB/s
        let speed_kb = speed_bytes / 1024.0;

        debug!(
            "速度测试完成: {} -> {} ({:.2} KB/s)",
            interface, test_url, speed_kb
        );

        Ok(speed_kb)
    }

    /// 获取最佳接口
    pub fn get_best_interface<'a>(
        &self,
        scores: &'a [InterfaceScore],
    ) -> Option<&'a InterfaceScore> {
        scores.first()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_calculation() {
        let results = vec![
            TestResult {
                interface: "eth0".to_string(),
                target: "8.8.8.8".to_string(),
                reachable: true,
                latency_ms: Some(10.0),
                packet_loss: Some(0.0),
                download_speed: Some(1024.0),
                tested_at: chrono::Local::now(),
            },
            TestResult {
                interface: "eth0".to_string(),
                target: "1.1.1.1".to_string(),
                reachable: true,
                latency_ms: Some(15.0),
                packet_loss: Some(0.0),
                download_speed: Some(2048.0),
                tested_at: chrono::Local::now(),
            },
        ];

        let tester = NetworkTester::new(5, 4);
        let scores = tester.calculate_scores(&results);

        assert_eq!(scores.len(), 1);
        assert_eq!(scores[0].interface, "eth0");
        assert_eq!(scores[0].reachable_count, 2);
    }
}

# OpenWrt 路由监控工具 (Routes Monitor)

> **Copyright (c) 2026 Hikaru (i@rua.moe)**  
> **All rights reserved.**  
> **Licensed under CC BY-NC 4.0 - Attribution required, Commercial use prohibited**

一个用 Rust 编写的智能路由监控工具，专为 OpenWrt 路由器设计，能够自动测试多个网络接口的连接质量，并智能切换到最佳网络接口。

## ✨ 功能特性

- 🔍 **智能监控**: 定期测试所有配置的网络接口到目标 IP 的连接性
- ⚡ **速度测试**: 支持下载速度测试，综合评估网络质量
- 🎯 **自动切换**: 根据测试结果自动切换到最佳网络接口
- 🛡️ **故障转移**: 支持连续失败阈值，避免频繁切换
- 📊 **详细日志**: 提供详细的测试结果和切换日志
- ⚙️ **灵活配置**: 通过 TOML 配置文件轻松自定义监控策略
- 🚀 **高性能**: 使用异步并发测试，快速完成检查
- 🔧 **OpenWrt 集成**: 原生支持 OpenWrt 的路由表和策略路由
- 📝 **UCI 路由管理**: 自动管理 `/etc/config/network` 中的静态路由配置

## 📋 系统要求

- OpenWrt 路由器
- Rust 1.70+ (编译时)
- 已安装的工具:
  - `ip` (iproute2)
  - `ping`
  - `curl` (可选，用于速度测试)
  - `uci` (OpenWrt 配置工具)

## 🚀 快速开始

### 编译

```bash
# 在开发机器上编译
cargo build --release

# 交叉编译到 OpenWrt (以 mipsel 为例)
# 需要先安装交叉编译工具链
rustup target add mipsel-unknown-linux-musl
cargo build --release --target mipsel-unknown-linux-musl
```

### 安装

```bash
# 将编译好的二进制文件复制到 OpenWrt 路由器
scp target/release/routes-monitor root@192.168.1.1:/usr/bin/

# 复制配置文件
scp config.toml root@192.168.1.1:/etc/routes-monitor/

# SSH 登录路由器
ssh root@192.168.1.1

# 赋予执行权限
chmod +x /usr/bin/routes-monitor
```

### 配置

编辑配置文件 `/etc/routes-monitor/config.toml`:

```toml
[global]
check_interval = 60          # 检查间隔（秒）
timeout = 5                  # 超时时间（秒）
concurrent_tests = 4         # 并发测试数量
failure_threshold = 3        # 连续失败阈值
log_level = "info"          # 日志级别
auto_switch = true          # 是否自动切换
manage_uci_routes = true    # 是否管理 UCI 静态路由

# 配置你的网络接口
[[interfaces]]
name = "eth0"
display_name = "有线网络"
priority = 1
enabled = true
table_id = 100
gateway = "192.168.1.1"

[[interfaces]]
name = "wlan0"
display_name = "WiFi 网络"
priority = 2
enabled = true
table_id = 101
gateway = "192.168.2.1"

# 配置监控目标
[[targets]]
address = "8.8.8.8"
description = "Google DNS"
test_url = "http://speedtest.tele2.net/1MB.zip"
weight = 1.0

# 关键服务器 - 启用 UCI 静态路由管理
[[targets]]
address = "45.128.210.197/32"
description = "生产服务器"
weight = 2.0
manage_as_static_route = true  # 自动在 UCI 中创建/更新路由
```

### 运行

```bash
# 直接运行
routes-monitor

# 指定配置文件路径
ROUTES_MONITOR_CONFIG=/path/to/config.toml routes-monitor

# 后台运行
nohup routes-monitor > /var/log/routes-monitor.log 2>&1 &
```

### 设置为系统服务

创建 init.d 脚本 `/etc/init.d/routes-monitor`:

```bash
#!/bin/sh /etc/rc.common

START=99
STOP=10

USE_PROCD=1
PROG=/usr/bin/routes-monitor

start_service() {
    procd_open_instance
    procd_set_param command $PROG
    procd_set_param respawn
    procd_set_param stdout 1
    procd_set_param stderr 1
    procd_close_instance
}
```

启用服务:

```bash
chmod +x /etc/init.d/routes-monitor
/etc/init.d/routes-monitor enable
/etc/init.d/routes-monitor start
```

## 📖 配置说明

### 全局配置 (`[global]`)

| 参数 | 类型 | 说明 | 默认值 |
|------|------|------|--------|
| `check_interval` | 整数 | 检查间隔（秒） | 60 |
| `auto_switch` | 布尔 | 是否自动切换接口 | true |
| `switch_threshold` | 浮点 | 切换阈值（评分差异） | 20.0 |
| `max_failures` | 整数 | 连续失败多少次后切换接口 | 3 |
| `manage_uci_routes` | 布尔 | 是否管理 UCI 静态路由 | false |
| `auto_switch` | 布尔 | 是否启用自动切换 | true |

### 网络配置 (`[network]`)

| 参数 | 类型 | 说明 | 默认值 |
|------|------|------|--------|
| `ping_timeout` | 整数 | Ping 超时时间（秒） | 5 |
| `ping_count` | 整数 | Ping 包数量 | 4 |
| `speed_test_timeout` | 整数 | 速度测试超时（秒） | 10 |

### 接口配置 (`[[interfaces]]`)

| 参数 | 类型 | 说明 | 必需 |
|------|------|------|------|
| `name` | 字符串 | 接口名称 (如 eth0, wlan0) | ✓ |
| `display_name` | 字符串 | 显示名称 | ✓ |
| `priority` | 整数 | 优先级（数字越小优先级越高） | ✓ |
| `enabled` | 布尔 | 是否启用此接口 | ✓ |
| `table_id` | 整数 | 路由表 ID（用于策略路由） | ✗ |
| `gateway` | 字符串 | 网关地址（留空则自动获取） | ✗ |

### 目标配置 (`[[targets]]`)

| 参数 | 类型 | 说明 | 必需 |
|------|------|------|------|
| `address` | 字符串 | IP 地址或域名 | ✓ |
| `description` | 字符串 | 描述 | ✓ |
| `test_url` | 字符串 | 速度测试 URL（可选） | ✗ |
| `weight` | 浮点数 | 权重（影响评分） | ✓ |
| `manage_as_static_route` | 布尔 | 是否作为 UCI 静态路由管理 | ✗ |

## 🔍 工作原理

### 核心监控流程

1. **监控循环**: 按配置的间隔定期检查所有启用的接口
2. **并发测试**: 对每个接口并发测试到所有目标的连接性
3. **速度评估**: 如果配置了 `test_url`，进行下载速度测试
4. **评分计算**: 
   - 可达性占 40%
   - 延迟占 30%
   - 速度占 30%
5. **智能切换**: 
   - 如果当前接口不是最佳接口，失败计数 +1
   - 达到失败阈值后切换到最佳接口
   - 切换后重置失败计数

### 路由管理流程

6. **策略路由管理**:
   - 清除旧的路由规则（优先级 100-999）
   - 设置新接口的策略路由
   - 更新默认网关
   - 刷新路由缓存

7. **UCI 静态路由管理**（当 `manage_uci_routes = true` 时）:
   - 更新现有静态路由的接口字段
   - 为标记的目标创建新的静态路由
   - 执行 `uci commit network`
   - 执行 `/etc/init.d/network reload`

详见 [UCI 路由管理文档](docs/UCI_ROUTES.md)

## 📊 日志示例

```
[2026-01-04 10:00:00] INFO  开始监控循环...
[2026-01-04 10:00:00] INFO  ==================== 第 1 次检查 ====================
[2026-01-04 10:00:00] INFO  开始测试 3 个接口...
[2026-01-04 10:00:05] INFO  测试结果汇总:
接口            可达目标      平均延迟(ms)    平均速度(KB/s)  评分      
----------------------------------------------------------------------
eth0            6            12.50           1024.00         85.30     
wlan0           6            25.00           512.00          72.15     
ppp0            5            50.00           256.00          58.45     

[2026-01-04 10:00:05] INFO  最佳接口: eth0 (评分: 85.30)
[2026-01-04 10:00:05] INFO  当前接口 wlan0 已连续 1 次非最佳 (阈值: 3)
```

## ⚠️ 注意事项

1. **权限要求**: 需要 root 权限才能修改路由表和 UCI 配置
2. **测试影响**: 频繁的测试可能消耗带宽，建议适当调整检查间隔
3. **切换稳定性**: 建议设置合理的失败阈值，避免频繁切换
4. **备份配置**: 切换前建议备份当前路由配置和 UCI 配置
5. **防火墙**: 确保防火墙允许 ping 和 curl 测试流量
6. **UCI 路由**: 启用 UCI 管理前请备份 `/etc/config/network`

## 🔧 故障排查

### 程序无法启动

- 检查配置文件路径和格式
- 验证所有必需字段是否填写
- 查看日志输出获取详细错误信息

### 无法切换接口

- 确认以 root 权限运行
- 检查接口名称是否正确
- 验证网关配置是否有效
- 查看 `ip route` 和 `ip rule` 输出

### UCI 路由管理问题

- 检查是否有 root 权限
- 验证 `uci` 命令是否可用
- 查看 `/etc/config/network` 文件权限
- 使用 `uci show network` 验证配置
- 详见 [UCI 路由管理文档](docs/UCI_ROUTES.md)

### 测试总是失败

- 验证目标 IP 是否可达
- 检查防火墙规则
- 增加超时时间
- 查看网络接口状态

## 🛠️ 开发

### 项目结构

```
routes-monitor/
├── src/
│   ├── main.rs          # 主程序入口
│   ├── config.rs        # 配置管理
│   ├── network.rs       # 网络测试
│   └── openwrt.rs       # OpenWrt 路由控制（包含 UCI 管理）
├── docs/                # 详细文档
├── init.d/              # OpenWrt init 脚本
├── config.toml          # 配置文件示例
├── Cargo.toml           # Rust 项目配置
└── README.md            # 项目文档
```

### 运行测试

```bash
cargo test
```

### 代码格式化

```bash
cargo fmt
```

### 静态分析

```bash
cargo clippy
```

## 📝 许可证

本项目采用 **Creative Commons Attribution-NonCommercial 4.0 International (CC BY-NC 4.0)** 许可证。

### 使用限制

- ✅ **允许**: 个人使用、学习、修改、分享
- ❌ **禁止**: 商业使用（需联系授权）
- 🎯 **必须**: 保留版权声明和作者署名

### 版权声明

```
Copyright (c) 2026 Hikaru (i@rua.moe)
All rights reserved.

This software is licensed under CC BY-NC 4.0
Attribution required, Commercial use prohibited
```

### 商业授权

如需商业使用，请联系: **i@rua.moe**

详见 [LICENSE](LICENSE) 文件。

## 🤝 贡献

欢迎提交 Issue 和 Pull Request！

## 📮 联系方式

- **作者**: Hikaru
- **邮箱**: i@rua.moe
- **Issues**: [GitHub Issues](https://github.com/HikaruChang/RoutesMonitor/issues)

---

**注意**: 本工具会修改系统路由表，请在测试环境中充分验证后再用于生产环境。

#!/bin/bash
# 部署脚本 - 自动部署到 OpenWrt 路由器

set -e

# 配置
ROUTER_IP="${ROUTER_IP:-192.168.1.1}"
ROUTER_USER="${ROUTER_USER:-root}"
BINARY="release/routes-monitor"
CONFIG="release/config.toml"
INIT_SCRIPT="release/routes-monitor.init"

echo "=========================================="
echo "  部署到 OpenWrt 路由器"
echo "=========================================="
echo "路由器地址: $ROUTER_IP"
echo "用户名: $ROUTER_USER"
echo ""

# 检查文件是否存在
if [ ! -f "$BINARY" ]; then
    echo "错误: 未找到编译好的二进制文件: $BINARY"
    echo "请先运行 ./build.sh 进行编译"
    exit 1
fi

echo "正在部署文件..."

# 创建目录
echo "创建配置目录..."
ssh "$ROUTER_USER@$ROUTER_IP" "mkdir -p /etc/routes-monitor"

# 复制文件
echo "复制二进制文件..."
scp "$BINARY" "$ROUTER_USER@$ROUTER_IP:/usr/bin/routes-monitor"

echo "复制配置文件..."
scp "$CONFIG" "$ROUTER_USER@$ROUTER_IP:/etc/routes-monitor/config.toml"

echo "复制 init.d 脚本..."
scp "$INIT_SCRIPT" "$ROUTER_USER@$ROUTER_IP:/etc/init.d/routes-monitor"

# 设置权限
echo "设置权限..."
ssh "$ROUTER_USER@$ROUTER_IP" "chmod +x /usr/bin/routes-monitor"
ssh "$ROUTER_USER@$ROUTER_IP" "chmod +x /etc/init.d/routes-monitor"

# 询问是否启动服务
echo ""
read -p "是否立即启动服务? (y/n) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "启用并启动服务..."
    ssh "$ROUTER_USER@$ROUTER_IP" "/etc/init.d/routes-monitor enable"
    ssh "$ROUTER_USER@$ROUTER_IP" "/etc/init.d/routes-monitor start"
    
    echo ""
    echo "服务已启动!"
    echo ""
    echo "查看日志:"
    echo "  ssh $ROUTER_USER@$ROUTER_IP 'logread | grep routes-monitor'"
    echo ""
    echo "停止服务:"
    echo "  ssh $ROUTER_USER@$ROUTER_IP '/etc/init.d/routes-monitor stop'"
fi

echo ""
echo "=========================================="
echo "  部署完成"
echo "=========================================="

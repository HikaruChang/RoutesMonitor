#!/bin/bash
# 构建脚本 - 用于交叉编译到 OpenWrt

set -e

echo "=========================================="
echo "  OpenWrt 路由监控工具 - 构建脚本"
echo "=========================================="

# 配置
TARGET_ARCH="${TARGET_ARCH:-mipsel-unknown-linux-musl}"
BUILD_TYPE="${BUILD_TYPE:-release}"

echo "目标架构: $TARGET_ARCH"
echo "构建类型: $BUILD_TYPE"
echo ""

# 检查 Rust 是否安装
if ! command -v cargo &> /dev/null; then
    echo "错误: 未找到 cargo，请先安装 Rust"
    exit 1
fi

# 检查目标架构是否已安装
echo "检查目标架构..."
if ! rustup target list --installed | grep -q "$TARGET_ARCH"; then
    echo "安装目标架构: $TARGET_ARCH"
    rustup target add "$TARGET_ARCH"
else
    echo "目标架构已安装"
fi

# 编译
echo ""
echo "开始编译..."
if [ "$BUILD_TYPE" = "release" ]; then
    cargo build --release --target "$TARGET_ARCH"
    BINARY_PATH="target/$TARGET_ARCH/release/routes-monitor"
else
    cargo build --target "$TARGET_ARCH"
    BINARY_PATH="target/$TARGET_ARCH/debug/routes-monitor"
fi

# 检查编译结果
if [ -f "$BINARY_PATH" ]; then
    echo ""
    echo "编译成功!"
    echo "二进制文件: $BINARY_PATH"
    
    # 显示文件大小
    SIZE=$(du -h "$BINARY_PATH" | cut -f1)
    echo "文件大小: $SIZE"
    
    # 创建发布目录
    RELEASE_DIR="release"
    mkdir -p "$RELEASE_DIR"
    
    # 复制文件
    cp "$BINARY_PATH" "$RELEASE_DIR/"
    cp config.toml "$RELEASE_DIR/"
    cp routes-monitor.init "$RELEASE_DIR/"
    cp README.md "$RELEASE_DIR/"
    
    # 如果安装了 strip，减小二进制文件大小
    if command -v strip &> /dev/null; then
        echo "优化二进制文件大小..."
        strip "$RELEASE_DIR/routes-monitor"
        STRIPPED_SIZE=$(du -h "$RELEASE_DIR/routes-monitor" | cut -f1)
        echo "优化后大小: $STRIPPED_SIZE"
    fi
    
    echo ""
    echo "发布文件已准备在: $RELEASE_DIR/"
    echo ""
    echo "部署步骤:"
    echo "1. 复制到路由器:"
    echo "   scp $RELEASE_DIR/routes-monitor root@<路由器IP>:/usr/bin/"
    echo "   scp $RELEASE_DIR/config.toml root@<路由器IP>:/etc/routes-monitor/"
    echo "   scp $RELEASE_DIR/routes-monitor.init root@<路由器IP>:/etc/init.d/routes-monitor"
    echo ""
    echo "2. SSH 登录路由器并执行:"
    echo "   chmod +x /usr/bin/routes-monitor"
    echo "   chmod +x /etc/init.d/routes-monitor"
    echo "   /etc/init.d/routes-monitor enable"
    echo "   /etc/init.d/routes-monitor start"
    
else
    echo "编译失败!"
    exit 1
fi

echo ""
echo "=========================================="
echo "  构建完成"
echo "=========================================="

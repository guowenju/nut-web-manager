# NUT Web Manager

NUT Web Manager 是一个面向家庭局域网的 NUT Web 管理平台，通过 SSH 管理 Debian、Proxmox VE 和 Proxmox Backup Server 主机。

它可以自动检测和安装 NUT、扫描 USB UPS、生成并应用 Server/Client 配置、设置自动关机策略，并在 Web 页面中展示 UPS 和保护链路状态。

NWM 只负责管理配置，不参与实时掉电决策。配置完成后，即使 NWM 或 Docker 主机停止运行，各主机上的 NUT 和 `upsmon` 仍会独立执行掉电保护。

## 支持范围

- Debian 13
- Proxmox VE 9.x
- Proxmox Backup Server 4.x
- 一台连接 USB UPS 的 NUT Server
- 多台 NUT Client
- NUT TCP 3493 局域网数据共享

> 当前只在`山特 TG-BOX 850`测试和使用

## 项目预览

![项目预览](https://raw.githubusercontent.com/guowenju/nut-web-manager/main/docs/images/preview.png)

## 工作方式

```text
USB UPS
   │
   ▼
NUT Server
   ├── 本机 primary upsmon
   ├── NUT Client：PVE / PBS / Debian
   └── 局域网内其它 NUT 客户端
```

## 部署

创建一个空目录，并将下面内容保存为 `compose.yaml`：

```yaml
services:
  nut-web-manager:
    image: guowenju/nut-web-manager:latest
    container_name: nut-web-manager
    restart: unless-stopped
    environment:
      NWM_BIND_ADDRESS: 0.0.0.0:8080
      NWM_DATA_DIR: /data
      NWM_ADMIN_USERNAME: ${NWM_ADMIN_USERNAME:-admin}
      NWM_ADMIN_PASSWORD: ${NWM_ADMIN_PASSWORD:-admin}
    ports:
      - "8080:8080"
    volumes:
      - ./data:/data
    stop_grace_period: 30s
```

## 安全边界

本项目面向可信家庭局域网：

- 不要将 Web 端口或 NUT TCP 3493 直接暴露到互联网。
- NWM 使用 root SSH 管理目标主机。
- NWM 不修改目标主机防火墙、软件源、PVE/PBS 网络或虚拟机配置。
- 删除 Host 只删除 NWM 本地记录，不清理远端公钥和配置。

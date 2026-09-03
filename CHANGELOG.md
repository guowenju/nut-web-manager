# Changelog

格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，并遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [Unreleased]

## [0.2.1](https://github.com/guowenju/nut-web-manager/compare/v0.2.0...v0.2.1) - 2026-09-03

### Fixed

- 修复多个后台采集器并发写入 SQLite 时频繁出现 `database is locked` 的问题。

## [0.2.0](https://github.com/guowenju/nut-web-manager/compare/v0.1.0...v0.2.0) - 2026-09-02

### Added

- 新增独立的“UPS 监控”模块，通过只读 NUT TCP 协议接入多个标准 NUT Server，并自动发现各数据源下的 UPS。

## [0.1.0](https://github.com/guowenju/nut-web-manager/commits/v0.1.0) - 2026-08-30

### Added

- 提供面向家庭局域网的 Web 管理界面。
- 支持管理 Debian 13、Proxmox VE 9.x 和 Proxmox Backup Server 4.x 主机。

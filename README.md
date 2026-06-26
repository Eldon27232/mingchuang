# kuake-fuckyou

整治国产流氓软件的 Windows 11 桌面治理工具。

## 定位

针对夸克网盘、酷狗音乐、123 云盘、百度网盘、搜狗输入法等会**塞快捷方式 / 抢文件关联 / 常驻保活 / 偷加自启动**的客户端,提供精准、可逆、一键化的反制手段。

不是杀软,不是清理大师,不当流氓软件——**一次性执行、绝不常驻**,所有破坏性操作前自动快照、可一键还原。

## 核心原则

1. **绝不常驻后台** —— 工具一次跑完即退出,不开机自启、不装服务、不挂托盘。
2. **可逆优先** —— 任何写注册表 / 改 ACL / 停服务的操作,前置自动快照,生成还原包,可一键回滚。
3. **dry-run 先于执行** —— 破坏性动作前先列出"将改动什么",用户勾选确认才执行。
4. **白名单护栏** —— 系统关键键、Defender / Update / explorer / 系统文件夹硬性拒改,代码层禁止穿透。
5. **诚实降级** —— 做不到的事(如 Windows UCPD 保护下的 http/https/.pdf 关联硬锁)明说,不假装。

## 功能(v1 范围,2026-06-27 定稿)

### 安全地基(P0)
- 操作前自动快照 + 一键还原
- dry-run 预览
- 系统关键项白名单护栏
- 软件画像档案(`profiles/*.json`,规则与代码解耦)
- 操作日志时间线
- 首启杀软排除引导(把工具加进白名单,不关杀软)

### 核心治理(P1)
- 自愈快捷方式清除 + "此电脑"命名空间清理 + 右键菜单清理
- 保活进程/服务一键禁用 + 自启清理
- **激进进程清理(用户提议)** —— 勾选模式:除系统/驱动/托盘/前台用户应用之外的进程,全 kill + 全禁自启动。带分类预览。

### 关联治理(P2)
- 媒体 / 压缩格式默认程序设置 + ACL 一次性硬封锁(打包 `SetUserFTA`,不自写哈希)
- 默认浏览器 / 主页劫持修复
- UCPD 状态感知与诚实降级(http/https/.pdf 走组策略路径或明示无法锁定)

### 不做(明确划线)
- 出站联网封锁 — 火绒等杀软已覆盖。
- 弹窗 / 推广窗口拦截 — 火绒主场。
- 定时巡检 / 偷改告警 — 违反"不常驻"原则。
- 保活实时溯源 — 改为"用户提案 → 本地复现 → 写进画像档案"的迭代模式。
- 系统遥测关停 — 与"治国产软件"主线偏。
- 社区规则在线订阅 — v1 仅内置 `profiles/` 规则,后置。

## 技术栈(规划)

- 主力:Rust + Tauri 2
- 注册表:`windows-registry` crate
- 服务:`windows-service` / Win32 SCM
- 计划任务:Win32 TaskScheduler (COM)
- ACL:Win32 Security Authorization
- 关联写入:打包调用 `SetUserFTA.exe`(不自写 UserChoice 哈希,微软改算法时换版本即可)
- 提权:主 UI `asInvoker`,真正写 HKLM / 停服务时按需 UAC 拉特权 helper 跑完即退;**不装常驻 SYSTEM 服务**
- 打包:MSI (WiX) 或签名 portable exe,避开 NSIS

## 目录约定

```
.
├── README.md               本文件
├── .gitignore
├── profiles/               软件画像档案(规则与代码解耦)
│   ├── SCHEMA.md           画像 schema 说明
│   ├── 123pan.json         123 云盘(本机已实测,完整指纹)
│   └── baidu-netdisk.json  百度网盘(本机已实测)
├── scripts/
│   └── diagnostics/        调研/取证脚本(不进产品,本地分析用)
│       ├── monitor-123-namespace.ps1   命名空间复原溯源(已定案:复原方为 explorer 自愈)
│       └── scan-processes.ps1          进程/服务分类扫描(为激进清理规则取证)
└── src-tauri/              Rust + Tauri 主工程(尚未初始化)
```

## 已验证的关键发现(2026-06-27)

1. **"此电脑"伪文件夹的彻底清理 = 两条 `reg delete`**
   - `HKCU\...\Explorer\MyComputer\NameSpace\{CLSID}` 子键
   - `HKCU\Software\Classes\CLSID\{CLSID}` 本体
   - 实测:reg delete 后服务重启不重写,explorer 也不复原。
2. **手动"删除"会复原的真凶是 `explorer.exe` 自己**,不是 123 服务。
   - UI 删除走 Shell API → explorer 内部"此电脑"一致性自愈机制 → 写回 NameSpace。
   - 命令行 `reg delete` 绕开 Shell,explorer 不感知,所以不会复原。
3. **`UserChoice` 关联锁定**:UCPD 当前只保护 `http`/`https`/`.pdf`,媒体 / 压缩格式可正常写。
4. **杀软策略**:对 Defender 加排除项(`Add-MpPreference -ExclusionPath`)而非关杀软。Tamper Protection 会自动重开杀软,关了也白关。

## 当前阶段

骨架初始化 + 内置 2 份画像(123 云盘 / 百度网盘)+ 2 份诊断脚本。Rust + Tauri 工程下一轮起。

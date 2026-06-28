# 内部文档

我自己看的, 记踩过的坑 + 关键决策 + 调研结论。客户不需要看, README.md 没这些。

## 已验证的关键发现

### "此电脑" 伪文件夹的清理 = 两条 `reg delete`

- `HKCU\...\Explorer\MyComputer\NameSpace\{CLSID}` 子键
- `HKCU\Software\Classes\CLSID\{CLSID}` 本体

实测: reg delete 后服务重启不重写, explorer 也不复原。

### 手动 "删除" 会复原的真凶是 `explorer.exe` 自己

不是 123 服务在背后写。

- UI 删除走 Shell API → explorer 内部 "此电脑" 一致性自愈机制 → 写回 NameSpace
- 命令行 `reg delete` 绕开 Shell, explorer 不感知, 所以不会复原

所以工具走 reg 路径就行, 不需要监控/抢占。

### UserChoice 关联锁定

- UCPD 当前只保护 `http` / `https` / `.pdf`, 媒体 / 压缩格式可正常写
- 自己 port hash 算法做过一次, 算错了把用户关联全清零 (commit 1eaa94b
  紧急回退). 教训: 不要自己写, 接入 DanysysTeam/PS-SFTA (MIT, 社区 5+ 年验证)
- 调用方式: PowerShell 单次 spawn, stdin 喂 ext 列表, stdout 收
  OK/ERR 一行一个。批处理 30 个 ext ~5s (单 spawn ~75s)
- patch 掉 SFTA.ps1 两处慢点: Write-RequiredApplicationAssociationToasts
  (枚举 ~92 个 RegisteredApps, 2s/call) + Update-RegistryChanges
  (Add-Type 编 C# 每次, Rust 端最后统一 SHChangeNotify)

### 杀软策略

不关杀软, 加排除项: `Add-MpPreference -ExclusionPath`。
Tamper Protection 会自动重开杀软, 关了也白关。

### 应用列表 "主动注册关联程序" 的判定

只有挂在 `HKLM/HKCU\Software\RegisteredApplications` 下 + Capabilities\\FileAssociations
有效的应用算注册。Windows 默认应用面板用这个口径。

实测本机 107 个应用中只 13 个真正注册 (Office 全套 / Chrome / Edge / Bandizip /
完美解码 / 夸克 / 网易邮箱大师)。7-Zip / VLC / mpv / Notepad++ 这类便携工具
不注册, 不会出现在 Windows 默认应用面板, 但能被"打开方式"菜单看到。

所以 UI 加"主动注册" toggle 是 *补充* 视图, 默认显示全部, 想限定就开它。

## 关键架构决策

### 为什么 sentry 单独 binary 而不是 lib 内 tokio task

- 长期运行进程独立崩了不影响 GUI
- 用户可以单独关 sentry (RUN_KEY 移除 + stop_requested 标志)
- 不需要保持 webview 内存常驻

### 为什么 sentry ↔ GUI 走共享文件而不是 named pipe

文件交换够用:
- sentry 1s 写一次 state.json
- GUI 3s 读一次
- control.json 反向通道

工程简单 (Drop, atomic rename 就行). pipe 要处理重连、断包、kill 后重启 handshake。

### updater 信任链

- 私钥本机 `~/.tauri/mingchuang.key` (无密码会被 tauri CLI 死锁在
  password prompt, 必须给个密码; 我用 mingchuang2026, 文档里也写明 — 反正
  仓库公开, 密码本身不是机密, 关键是私钥文件不进 git)
- 公钥编译期进 `tauri.conf.json -> plugins.updater.pubkey`
- 发布 .msi 同时附 .msi.sig + latest.json
- 客户端拉 latest.json → 校验 .msi.sig → /passive 安装 → 重启

### 桌面快捷方式不再扫

用户明确不要。所有 govern_scan_shortcuts 相关 UI/逻辑保留 (历史诊断有用),
但不在巡检循环里。

## 协议 / 文件格式

### sentry 共享文件

`%LOCALAPPDATA%\mingchuang\sentry\`:

- `state.json` — sentry 写, GUI 读, 当前状态
- `control.json` — GUI 写, sentry 读, 控制位 (暂停 / 停止 / 巡检开关)
- `user.json` — 用户白名单 (PCDN 不告警的进程)
- `events-YYYY-MM-DD.jsonl` — 告警日志, 两种 schema 同存:
  * PCDN: `{ts, pid, image_name, up_bps}`
  * 巡检/偷改: `{ts, kind, category, label, detail}`
  GUI 读时按字段在不在分流
- `inspection-baseline.json` — 上次定时巡检的全量快照, 用于下次 diff

### ProgId 命名

`KuakeFuckyou.<exe_stem>` (停留在历史名, rename 后会变 `Mingchuang.<stem>` —
等 rename 那个 commit 落了, 老用户首次升级会看到旧 ProgId 还在
HKCU\Software\Classes, 但新设的关联走新 ProgId。冲突的解决留 TODO)

### Marketing tone for README

我自己第一人称写, 不写 "我们" "团队"。
不写 "本产品" "本工具", 直接 "明窗" 或 "它"。
不写 "提供" "支持", 直接 "能做" "干掉"。

## 调研脚本

`scripts/diagnostics/`:

- `monitor-123-namespace.ps1` — 命名空间复原溯源 (已定案: 复原方为 explorer 自愈)
- `scan-processes.ps1` — 进程/服务分类扫描 (为激进清理规则取证)

这些只在调研阶段跑过, 没进产品。留着是因为换台机器复现/调试有用。

## 当前 v1 范围明确不做

(原 README 的 "不做" 段移到这里, 客户不需要看到这些 negative space)

- 出站联网封锁 — 火绒等杀软已覆盖
- 弹窗 / 推广窗口拦截 — 火绒主场
- 保活实时溯源 — 改为"用户提案 → 本地复现 → 写进画像档案"的迭代模式
- 系统遥测关停 — 与 "治国产软件" 主线偏
- 社区规则在线订阅 — v1 仅内置 `profiles/` 规则, 后置
- 桌面快捷方式管理 — 用户明确不要, 不进巡检

桌面快捷方式 / 出站封锁 等用火绒等通用工具更合适。明窗只做国产流氓软件 *特有*
的注册表 / 关联 / 命名空间这类活, 不重复造轮子。

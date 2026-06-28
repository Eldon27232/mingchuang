# 明窗

我自己写的, 用来收拾 Windows 11 上那些国产流氓客户端 — 夸克网盘、酷狗音乐、
123 云盘、百度网盘、搜狗输入法之类。

它们的套路就那几样:

- 往「我的电脑」塞自己的图标 (伪文件夹)
- 一键把默认打开方式抢成自己
- 后台进程怎么杀都活, 服务关了又开
- 偷偷加开机启动、计划任务
- 你手动改回来, 一会儿又被改回去

火绒能管的就不重复造轮子, 但火绒管不到上面这些 — 全是注册表 + 关联 +
COM 命名空间的活, 得专门挑出来治。明窗就干这个。

## 下载

最新版: https://github.com/Eldon27232/mingchuang/releases/latest

`.msi` 双击装就行, 装完桌面有快捷方式, 第一次跑会弹 UAC (它要改注册表)。
应用内会自己检查更新。

## 它能干什么

**一键体检 + 清理**
扫一下你的电脑, 列出"被偷塞的图标 / 后台保活进程 / 流氓自启项",
你勾哪个清哪个, 不勾不动。

**默认打开方式真·全自动**
选个应用 (PotPlayer / VLC / 7-Zip / 任意 exe), 给它分配文件类型,
媒体 / 压缩 / 图片这些直接锁住 — 不用跳系统设置手动确认, 也不会被改回去。

**定时巡检 + 偷改告警** (可选, 默认关)
开了之后后台每小时 (可调 15min/1h/6h/24h) 跑一遍扫描, 发现"图标又回来了"
或"默认应用又被改了"立刻通知你。两个开关独立, 不要可以不开。

**PCDN 监控** (可选, 默认关)
有些国产软件偷偷拿你家带宽当 P2P 中转 (PCDN). 开了之后任何非白名单进程
持续上传超 8 Mbps 会弹通知 + 一键 kill / 加白。

**所有破坏性操作前自动快照**
工具改的每一处注册表都留快照, 不满意一键还原。

## 它不会干什么

- **不常驻** (除非你主动开 PCDN 监控 / 定时巡检, 那两个是独立 sentry 进程,
  可以随时关)
- **不联网封锁、不弹窗拦截** — 火绒主场, 我不抢
- **不关你的杀软** — 杀软误报本工具的话, 给杀软加排除项 (`Add-MpPreference -ExclusionPath`)
- **不改 HKLM 系统关键键** — 有硬编码白名单挡着
- **不偷你的数据** — 这工具本身是开源的, 代码就在上面, 自己看

## 不行的地方

- 浏览器 (http/https) 和 PDF 关联 Windows 自身有 UCPD 防护, 锁不住,
  会引导你跳系统设置手动选
- 桌面快捷方式不管 (用其他工具或手动)
- 保活进程清理目前只对常见服务/计划任务有效, 一些深度变种打不到根 (用激进清理勾选)
- 装完会弹 UAC 是因为它真的要改注册表, 不弹就办不成事

## 反馈

GitHub Issues: https://github.com/Eldon27232/mingchuang/issues

碰到没清干净的软件、扫描误报、UI 卡顿都欢迎提。最好把 `%LOCALAPPDATA%\
mingchuang\snapshots\` 下的快照 ID 也附上, 我好复现。

## 自己编译

需要 Rust + Node:

```powershell
git clone https://github.com/Eldon27232/mingchuang.git
cd mingchuang
npm install
npm run tauri dev
```

发布构建 + 签名见 [docs/RELEASING.md](docs/RELEASING.md).

## 协议

MIT. 用了 [DanysysTeam/PS-SFTA](https://github.com/DanysysTeam/PS-SFTA) (MIT)
做 UserChoice 哈希计算, 源码在 `src-tauri/resources/SFTA.ps1`.

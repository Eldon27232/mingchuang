# 软件画像档案 schema

每份画像是一个独立 JSON 文件,描述一款流氓软件的**指纹**、**清理动作**、**回滚方式**。规则与代码解耦,新增/更新流氓软件只改 JSON,不动 Rust 代码。

## 顶层字段

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `id` | string | ✓ | 唯一 id,kebab-case,如 `123pan`、`baidu-netdisk` |
| `name` | string | ✓ | 显示名(中文) |
| `vendor` | string | ✓ | 厂商 |
| `category` | string | ✓ | 类别:`netdisk` / `music` / `video` / `ime` / `browser` / `cleaner` / `other` |
| `severity` | string | ✓ | 流氓程度:`critical` / `high` / `medium` / `low` |
| `tested_on` | object |   | 实测环境元数据(Windows 版本、软件版本、日期) |
| `fingerprints` | object | ✓ | 识别此软件的指纹集合 |
| `actions` | array  | ✓ | 治理动作清单(执行顺序即数组顺序) |
| `verify` | array  |   | 治理后验证项(用于工具自检"清理是否成功") |
| `notes` | string |   | 自由备注 |

## `fingerprints` 子字段

```jsonc
{
  "install_paths": ["C:\\Users\\*\\AppData\\Local\\123pan"],   // 安装目录通配
  "process_names": ["KuGou.exe"],                              // 进程名
  "service_names": ["123SyncCloud Maintenance Service"],       // Windows 服务名
  "task_names":    ["\\QuarkUpdater*"],                        // 计划任务路径通配
  "clsids":        ["{D5BE1ADA-...}"],                         // Shell 命名空间 CLSID
  "registry_keys": ["HKCU\\Software\\123pan"],                 // 软件自身的注册表根
  "shortcut_targets": ["123pan.exe"]                           // .lnk 指向的目标 exe 名
}
```

任一指纹命中即可识别为本软件;指纹用于:扫描分类、激进进程清理时判定是否流氓、卸载残留检测。

## `actions` 元素

每个动作描述"做什么 / 撤销什么 / 是否需要管理员":

```jsonc
{
  "kind": "reg-delete",                          // 动作类型,见下表
  "target": "HKCU\\Software\\...\\NameSpace\\{D5BE1ADA-...}",
  "reason": "删除 此电脑 里 123 云盘 伪文件夹",   // dry-run 时给用户看的人话
  "elevate": false,                              // 是否需要管理员
  "rollback": {                                  // 撤销方式
    "kind": "reg-import",
    "from": "snapshot://this-action"             // 从动作前快照恢复
  }
}
```

### 支持的 `kind`(逐步扩展)

| kind | 含义 | elevate 通常 | rollback |
|---|---|---|---|
| `reg-delete` | 删注册表键/值 | 视位置而定 | 从快照导入 |
| `reg-set` | 写注册表值 | 视位置而定 | 写回原值 |
| `reg-deny-acl` | 给注册表键加 Deny ACL(一次性硬封锁) | ✓ | 还原原 ACL |
| `service-stop` | 停服务 | ✓ | start |
| `service-disable` | 设服务启动类型为 Disabled | ✓ | 设回原 StartMode |
| `service-delete` | 删除服务(慎用) | ✓ | 重建(需保存原 PathName/Type/...) |
| `task-disable` | 禁用计划任务 | ✓ | 重新启用 |
| `task-delete` | 删除计划任务 | ✓ | 重建(需保存原 XML) |
| `file-delete` | 删文件(如 .lnk) | 视位置 | 从回收站/快照恢复 |
| `file-deny-acl` | 文件加 Deny ACL | 视位置 | 还原原 ACL |
| `process-kill` | 杀进程 | 视进程令牌 | 不可逆(进程是临时状态) |
| `firewall-block` | 加出站防火墙 Block 规则(v1 不做,占位) | ✓ | 删规则 |

## `verify` 元素

```jsonc
{
  "kind": "reg-not-exists",                                 // 或 reg-exists / file-not-exists / service-state
  "target": "HKCU\\...\\NameSpace\\{D5BE1ADA-...}",
  "description": "命名空间项已不存在"
}
```

工具执行完清理后逐项检查,任一失败 = 清理未生效,提示用户。

## 路径展开规则

- `%LOCALAPPDATA%`、`%APPDATA%`、`%PROGRAMDATA%`、`%PROGRAMFILES%`、`%PROGRAMFILES(X86)%` 按运行时环境变量展开。
- `C:\\Users\\*` 中的 `*` 在扫描时按当前用户展开,在指纹匹配时按通配。

## 风格约定

- 字段名一律 `snake_case`。
- 注释统一在 `notes` 或紧邻字段的 `// ...` 中(JSON5 风格仅用于说明文档,实际文件用纯 JSON)。
- 顺序:身份(id/name/vendor) → 元数据 → 指纹 → 动作 → 验证 → 备注。

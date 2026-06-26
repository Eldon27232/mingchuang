# 软件画像档案 schema

每份画像是一个独立 JSON 文件,描述一款流氓软件的**指纹**、**清理动作**、**回滚方式**。规则与代码解耦,新增/更新流氓软件只改 JSON,不动 Rust 代码。

> **schema 版本: v0.2**(2026-06-27 演化,新增 `code_sign_subjects` / MSI 标识符 / 模糊路径关键字 / 上报域名)

## 顶层字段

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `id` | string | ✓ | 唯一 id,kebab-case |
| `name` | string | ✓ | 显示名(中文) |
| `vendor` | string | ✓ | 厂商 |
| `category` | string | ✓ | 类别:`netdisk` / `music` / `video` / `ime` / `browser` / `cleaner` / `other` |
| `severity` | string | ✓ | 流氓程度:`critical` / `high` / `medium` / `low` |
| `tested_on` | object |   | 实测环境元数据 |
| `fingerprints` | object | ✓ | 识别此软件的指纹集合(见下表) |
| `actions` | array  | ✓ | 治理动作清单(执行顺序即数组顺序) |
| `verify` | array  |   | 治理后验证项 |
| `notes` | string |   | 自由备注 |

## `fingerprints` 子字段(v0.2 演化后)

### 强标识(优先用,稳定不易变)

| 字段 | 类型 | 说明 | 来源 / 取证手段 |
|---|---|---|---|
| `code_sign_subjects` | string[] | **代码签名主体 CN 名**(最稳定的指纹:目录会改、进程会变、证书主体多年不变) | `Get-AuthenticodeSignature` 的 `SignerCertificate.Subject` 取 `CN=` 字段 |
| `msi_product_code` | string |  MSI 安装的 ProductCode `{GUID}` | winget-pkgs manifest 或 `HKLM\...\Uninstall\{GUID}` |
| `msi_upgrade_code` | string | MSI 安装的 UpgradeCode `{GUID}` | winget-pkgs manifest |
| `clsids` | string[] | Shell 命名空间 / 右键扩展 CLSID(`{GUID}`) | 注册表 `HKCU/HKLM\Software\Classes\CLSID` |

### 路径/名称类指纹(易随版本变化,作为辅助)

| 字段 | 类型 | 说明 |
|---|---|---|
| `install_paths` | string[] | 精确安装目录(支持 `%LOCALAPPDATA%` 等环境变量) |
| `install_path_keywords` | string[] | **模糊**安装目录关键字(如 `\7654Browser\`),命中即可,无需精确(借鉴 SoftCnKiller) |
| `process_names` | string[] | 进程 exe 名(精确) |
| `service_names` | string[] | Windows 服务名 |
| `task_names` | string[] | 计划任务路径通配 |
| `registry_keys` | string[] | 软件自身的注册表根 |
| `shortcut_targets` | string[] | `.lnk` 指向的目标 exe 名 |

### 网络/行为类指纹(为 P2/P3 动作预留)

| 字段 | 类型 | 说明 |
|---|---|---|
| `report_domains` | string[] | 已知上报/广告域名(供未来 `hosts-block` 动作使用) |

**指纹优先级**:`code_sign_subjects` > `msi_product_code` > `clsids` > `service_names` > `install_paths` > `process_names` > `install_path_keywords`。

匹配规则:任一指纹命中即可识别为本软件;但**激进清理**时,工具会优先用强指纹(签名+MSI+CLSID)避免误杀。

## `actions` 元素

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

| kind | 含义 | elevate | rollback |
|---|---|---|---|
| `reg-delete` | 删注册表键/值 | 视位置 | 从快照导入 |
| `reg-set` | 写注册表值 | 视位置 | 写回原值 |
| `reg-deny-acl` | 给注册表键加 Deny ACL(硬封锁,实现"锁死") | ✓ | 还原原 ACL |
| `service-stop` | 停服务 | ✓ | start |
| `service-disable` | 设服务启动类型为 Disabled | ✓ | 设回原 StartMode |
| `service-delete` | 删除服务(慎用) | ✓ | 重建(需保存原 PathName/Type) |
| `task-disable` | 禁用计划任务 | ✓ | 重新启用 |
| `task-delete` | 删除计划任务 | ✓ | 重建(需保存原 XML) |
| `file-delete` | 删文件(如 `.lnk`) | 视位置 | 从回收站/快照恢复 |
| `file-deny-acl` | 文件加 Deny ACL | 视位置 | 还原原 ACL |
| `process-kill` | 杀进程 | 视进程令牌 | 不可逆 |
| `run-official-uninstaller` | 调用厂商 QuietUninstallString(winget-pkgs 提供) | 通常 ✓ | 不可逆 |
| `hosts-block` | 在 hosts 加屏蔽规则 *(P2)* | ✓ | 删规则 |
| `firewall-block` | 出站防火墙 Block 规则 *(P2,与火绒重合,可能不做)* | ✓ | 删规则 |

## `verify` 元素

```jsonc
{
  "kind": "reg-not-exists",     // 或 reg-exists / file-not-exists / service-state
  "target": "HKCU\\...",
  "description": "命名空间项已不存在"
}
```

## 路径展开规则

- `%LOCALAPPDATA%`、`%APPDATA%`、`%PROGRAMDATA%`、`%PROGRAMFILES%`、`%PROGRAMFILES(X86)%` 按运行时环境变量展开
- `C:\\Users\\*` 中的 `*` 在扫描时按当前用户展开

## 风格约定

- 字段名一律 `snake_case`
- 顺序:身份(id/name/vendor) → 元数据 → 指纹 → 动作 → 验证 → 备注
- JSON 不支持注释,说明放在 `notes` 字段

## schema 演化历史

- **v0.1**(2026-06-27 上午): 初版,基础指纹 + 动作
- **v0.2**(2026-06-27 下午): 调研开源数据库后演化:
  - 新增 `code_sign_subjects`(P0,Malware-Patch + SoftCnKiller 两大源都按此组织数据)
  - 新增 `msi_product_code` / `msi_upgrade_code`(P0,winget-pkgs 提供)
  - 新增 `install_path_keywords`(P1,模糊路径)
  - 新增 `report_domains`(P1,为 hosts-block 预留)
  - 新增 `actions.kind = run-official-uninstaller`(消费 winget 的 QuietUninstallString)

## 战略目标

`profiles/` 目录最终独立成 MIT 仓接受 PR,占住"中文 Windows 反流氓 · 结构化 · 宽松许可 · 持续维护"的 OSS 生态位空白(2026-06-27 调研结论)。

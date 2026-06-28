# 发布流程

本项目用 [tauri-plugin-updater](https://v2.tauri.app/plugin/updater/) 做自动更新, 走 GitHub Releases。每次发版需要:

1. bump 版本号
2. 本地 `tauri build` 出签名的 .msi
3. 在 GitHub Releases 创建一个 tag 对应的 release, 上传 .msi + .msi.sig
4. 上传 `latest.json` 清单 — 应用启动检查更新时就拉这个文件

---

## 前置: 签名密钥

私钥在本机 `~/.tauri/mingchuang.key` (无密码, 已加入 `.gitignore` `*.key`)。
**这个文件丢了或泄露 = 用户的"自动更新"就废了**。要么备份, 要么写代码更换公钥并强制用户手动重装。

公钥已经写进 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`, 不需要每次手动管。

签名通过环境变量 `TAURI_SIGNING_PRIVATE_KEY` 传给 build, 见下面。

## ⚠ WiX UpgradeCode (绝对不能改)

`bundle.windows.wix.upgradeCode = 25C0E97E-79F1-4F17-AC1A-CE898C3851AB`

Tauri 默认根据 `<exe名>.exe` 哈希算 UpgradeCode, **任何时候改了 Rust binary 名
(Cargo.toml [[bin]] name) UpgradeCode 都会变 — 新装包不再当成升级而是并行装,
开始菜单出现两份"明窗", 用户哭着来找你**。v0.0.1 就因为 rename 前后 bin 名
从 kuake-fuckyou 改成 mingchuang 撞过这个坑, 从 v0.0.7 起锁死这个 UUID。

如果不得不换 UpgradeCode (比如真要大版本切产品线), 老用户必须手动卸载老版,
不然永远并行。

---

## 每次发版步骤

### 1. bump 版本号

需要同时改两处:

- `src-tauri/Cargo.toml` 顶部 `version = "0.0.x"`
- `src-tauri/tauri.conf.json` 顶部 `"version": "0.0.x"`

`package.json` 不参与 updater 逻辑, 不用改 (但建议同步保持心智一致)。

### 2. 本地 build 签名的 .msi

```powershell
# 把私钥路径塞进环境变量, 然后 tauri build
$env:TAURI_SIGNING_PRIVATE_KEY = "$env:USERPROFILE\.tauri\mingchuang.key"
npx tauri build
```

产物 (路径相对项目根):

- `src-tauri/target/release/bundle/msi/明窗_0.0.x_x64_zh-CN.msi`
- `src-tauri/target/release/bundle/msi/明窗_0.0.x_x64_zh-CN.msi.sig`   ← 签名 (小文本文件)

如果只看到 `.msi` 没有 `.sig`, 是没设环境变量 / 没设 `bundle.createUpdaterArtifacts=true` (本项目 conf 已开)。

### 3. 创建 GitHub Release, 上传产物

```powershell
$ver = "0.0.x"   # 跟 Cargo.toml / tauri.conf.json 对齐
$msi = "src-tauri\target\release\bundle\msi\明窗_${ver}_x64_zh-CN.msi"
$sig = "$msi.sig"

gh release create "v$ver" $msi $sig `
  --title "v$ver" `
  --notes-file CHANGELOG-$ver.md
```

`--notes-file` 可以不传, 改用 `--notes "..."` 直接传字符串。这段 notes 会作为
"更新内容" 显示在用户的更新对话框里。

### 4. 写并上传 `latest.json`

这是 updater 实际拉取的清单。格式:

```json
{
  "version": "0.0.x",
  "notes": "更新说明 (会显示给用户)",
  "pub_date": "2026-06-28T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<.msi.sig 文件的全部内容, 一行 base64>",
      "url": "https://github.com/Eldon27232/mingchuang/releases/download/v0.0.x/明窗_0.0.x_x64_zh-CN.msi"
    }
  }
}
```

**⚠ 两个坑必须避开**:

1. **GitHub 会剥 asset 文件名里的中文** — 上传 `明窗_0.0.x_x64_zh-CN.msi` 后实际 asset 叫 `_0.0.x_x64_zh-CN.msi` (前缀下划线是被剥的"明窗"残骸). 解决: build 完先复制重命名成 ASCII 前缀 `Mingchuang_*`, 上传那一份, latest.json 的 url 也用同一名.

2. **PowerShell 5.1 `Set-Content -Encoding utf8` 写的是带 BOM 的 UTF-8** — tauri-updater 的 JSON parser 拒收带 BOM 的输入, 客户端报 "error decoding response body". 解决: 用 `[System.IO.File]::WriteAllText` + `UTF8Encoding($false)`.

生成 + 上传脚本 (粘贴到 PowerShell, 改 `$ver`):

```powershell
$ver = "0.0.x"
$stage = "$env:TEMP\mingchuang-v$ver"
New-Item -ItemType Directory -Force $stage | Out-Null

# 1) 复制 build 产物到 staging, 改 ASCII 前缀名 (避 GitHub 剥中文)
$msiSrc = "src-tauri\target\release\bundle\msi\明窗_${ver}_x64_zh-CN.msi"
$exeSrc = "src-tauri\target\release\bundle\nsis\明窗_${ver}_x64-setup.exe"
$msi = "$stage\Mingchuang_${ver}_x64_zh-CN.msi"
$exe = "$stage\Mingchuang_${ver}_x64-setup.exe"
Copy-Item $msiSrc $msi
Copy-Item "$msiSrc.sig" "$msi.sig"
Copy-Item $exeSrc $exe
Copy-Item "$exeSrc.sig" "$exe.sig"

# 2) 生成 latest.json 无 BOM
$sigContent = (Get-Content -Raw "$msi.sig").Trim()
$latest = [PSCustomObject]@{
  version = $ver
  notes = "更新说明"
  pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = [PSCustomObject]@{
    "windows-x86_64" = [PSCustomObject]@{
      signature = $sigContent
      url = "https://github.com/Eldon27232/mingchuang/releases/download/v$ver/Mingchuang_${ver}_x64_zh-CN.msi"
    }
  }
}
$json = $latest | ConvertTo-Json -Depth 5 -Compress
$latestPath = "$stage\latest.json"
# ⚠ 必须用 WriteAllText + UTF8Encoding(false), Set-Content -Encoding utf8 会带 BOM
[System.IO.File]::WriteAllText($latestPath, $json, [System.Text.UTF8Encoding]::new($false))

# 3) 上传
gh release create "v$ver" $msi "$msi.sig" $exe "$exe.sig" $latestPath `
  --title "v$ver" --notes "..." --repo Eldon27232/mingchuang
```

---

## 用户那一端会怎么走

1. 应用启动后, 用户点 "检查更新"
2. updater 插件请求 `https://github.com/Eldon27232/mingchuang/releases/latest/download/latest.json`
3. 比较 `latest.json` 里的 `version` 和当前 `Cargo.toml` 里的 `version`
4. 高了就提示有新版本, 显示 `notes`
5. 用户点 "下载并安装" → 后台下 .msi, 用公钥校验 .msi.sig, 通过则起 .msi 安装
6. 装完弹"已安装, 重启生效", 用户点重启

`installMode: "passive"` 意味着 .msi 用 `/passive` 模式运行 — 显示进度但不需要任何交互。

---

## 排错

| 现象 | 原因 |
|---|---|
| `tauri build` 没生成 .sig 文件 | 没设 `TAURI_SIGNING_PRIVATE_KEY`, 或 `bundle.createUpdaterArtifacts=false` |
| 客户端报 `signature verification failed` | `latest.json` 里 signature 字段拷错 (要 .sig 文件全部内容, 不只是 hash) |
| `latest.json` 404 | release 没上传 `latest.json`, 或 release tag 不是 latest (草稿/预发布) |
| 客户端 "URL did not return a valid update" | 检查 `latest.json` 的 url 字段是否能直接 wget 到 .msi |
| `~/.tauri/mingchuang.key` 丢了 | 重新 `tauri signer generate`, 公钥改进 tauri.conf.json, 必须发一版"手动重装公告"才能让老用户继续收到更新 |

---

## 一键脚本 (TODO)

后续可以把"bump version → build → 创 release → 写 latest.json → 上传"打成
`scripts/release.ps1`, 只需要传版本号一个参数。

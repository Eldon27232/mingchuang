# 画像待补 TODO (Malware-Patch 种子)

数据源: [the1812/Malware-Patch](https://github.com/the1812/Malware-Patch) `src/MalwarePatch/Certificates/`(MIT,239 个 .cer 文件,71 个一级 vendor,月级活跃)

**用法**: 跑 `scripts/tools/import-malware-patch.ps1`(待新增)拉取所有 .cer,用 `X509Certificate2` 解析得到 Subject CN,自动填进 `data/code-sign-subjects.json` 供 fuzzy matcher 用。或手工挑一个写画像 → 在下表打 ✓。

> 类别提示:`netdisk` 网盘 · `music` 音乐 · `video` 视频 · `browser` 浏览器 · `cleaner` 清理大师 · `download` 下载工具 · `news` 资讯 · `wallpaper` 壁纸 · `livestream` 直播 · `office` 办公 · `ime` 输入法 · `taobao` 电商 · `av` 杀软 · `driver` 驱动

## 已画像

| id | name | vendor | 状态 |
|---|---|---|---|
| `123pan` | 123 云盘 | 西安一二三云计算有限公司 | ✓ 实测 + EV 签名指纹 |
| `baidu-netdisk` | 百度网盘 | Beijing Duyou Science and Technology Co.,Ltd. | ✓ 实测 + 签名指纹 |
| `kugou` | 酷狗音乐 | Guangzhou Kugou Technology Co., Ltd. | ✓ 本机签名实测 |

## 待画像(71 vendor,按 Malware-Patch 文件名前缀)

### 网盘/下载

| vendor | 候选画像 id | 类别 | 备注 |
|---|---|---|---|
| `baidu` | `baidu-suite` | other | 百度全家桶(`baidu`/`baidu.cer`),含浏览器/下载/输入法等 |
| `baidu browser` | `baidu-browser` | browser | |
| `baidu download` | `baidu-download` | download | |
| `baidusp` | `baidusp` | other | 百度安装包推广 |
| `thunder` | `thunder` | download | 迅雷 |
| `xundu` | `xundu` | download | 迅读 PDF? 待查 |
| `kuaizip` | `kuaizip` | other | 快压 |

### 浏览器

| vendor | 候选画像 id | 备注 |
|---|---|---|
| `2345 browser` | `2345-browser` | 2345 加速浏览器 |
| `360 browser` | `360-browser` | 360 浏览器 |
| `kingsoft browser` | `kingsoft-browser` | 猎豹浏览器 |
| `sogou` | `sogou-browser` | 搜狗高速浏览器 |

### 音乐 / 视频 / 直播

| vendor | 候选画像 id | 备注 |
|---|---|---|
| `kugou` | (已画) | — |
| `kuwo` | `kuwo` | 酷我音乐 |
| `baofeng` | `baofeng` | 暴风影音 |
| `funshion` | `funshion` | 风行视频 |
| `qiyi` | `qiyi` | 爱奇艺 |
| `pptv` | `pptv` | PPTV 聚力 |
| `pplive` | `pplive` | PPTV/PPLive |
| `youku` | `youku` | 优酷 |
| `leshi` | `leshi` | 乐视 |
| `sohu` | `sohu-video` | 搜狐影音 |
| `huya` | `huya` | 虎牙直播 |
| `yy` | `yy` | YY 语音 |

### 清理/工具/优化

| vendor | 候选画像 id | 备注 |
|---|---|---|
| `360` | `360-suite` | 360 安全卫士全家桶 |
| `360 ludashi` | `360-ludashi` | 鲁大师 |
| `360 wallpaper` | `360-wallpaper` | 360 壁纸 |
| `360 sd` | `360-sd` | 360 杀毒 |
| `kingsoft` | `kingsoft-suite` | 金山全家桶 |
| `kingsoft wps` | `kingsoft-wps` | WPS 注:wps 本身合理,但其推广组件可治 |
| `driveTheLife` | `drive-the-life` | 驱动人生 |
| `rising` | `rising` | 瑞星 |
| `aogewei` | `aogewei` | 鸿合白板软件? 待查 |
| `2345` | `2345-suite` | 2345 软件管家/王牌浏览器 |
| `tencent` | `tencent-promo` | 腾讯系推广组件(不是 QQ 本体) |

### 资讯 / 头条 / 弹窗

| vendor | 候选画像 id | 备注 |
|---|---|---|
| `donfang toutiao` | `dongfang-toutiao` | 东方头条 |
| `fengqi` | `fengqi` | 蜂奇广告? 待查 |
| `higeshi` | `higeshi` | 待查 |
| `donfang` | `dongfang` | 同上 |

### 输入法 / 工具

| vendor | 候选画像 id | 备注 |
|---|---|---|
| `sogou` | `sogou-ime` | 搜狗输入法 |
| `aliwangwang` | `aliwangwang` | 阿里旺旺 |
| `taobao` | `taobao-suite` | 淘宝相关组件 |
| `qidian` | `qidian` | 起点读书 |
| `netease` | `netease-misc` | 网易杂项(其下含 cloudmusic/youdao,需分拆) |
| `netease youdao` | `netease-youdao` | 有道相关 |

### 不明 / 待查

`6789` / `7654 note` / `baishengtong` / `grid verse (format factory)` / `kuaizip` / `riyue` / `ruanmei` / `shabake` / `shaji` / `tingfengyu` / `tuling` / `windsoul` / `xingcheng` / `yunbiao` / `zhongcheng` —— 中文社区小众或恶意,补画像前需先调研其行为是否真流氓。

> `grid verse (format factory)` 是格式工厂,本身偏中性工具,推广组件需单独治。

## 战略动作

按调研建议,下面这两步具有最高 ROI:

1. **跑 `scripts/tools/import-malware-patch.ps1`** 把所有 .cer 的真实 Subject CN 一次性拉下来,生成 `data/code-sign-subjects.json`。Rust 端的扫描器即可在内存里 fuzzy 匹配本机所有 exe 的签名 CN,**识别覆盖范围一夜从 3 个跳到 71 个**(虽然没画像但能定位)。
2. **`profiles/` 目录独立成 MIT 仓接受 PR**,占住中文 Windows 反流氓 OSS 生态位空白。

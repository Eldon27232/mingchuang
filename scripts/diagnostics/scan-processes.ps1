<#
.SYNOPSIS
  扫描本机进程/服务/自启动项,按"系统 / 驱动 / 用户应用 / 服务驱动保活 / 可疑保活 / 未知"分类。
  为「激进进程清理」功能的分类规则取证,也为画像档案补充候选指纹。

.DESCRIPTION
  非破坏性,纯只读。同时:
    - 加载 ../../profiles/*.json 中的进程/服务/CLSID 指纹,命中的进程标记 [PROFILE:<id>]
    - 关联 Win32_Service 与进程 PID,标出"由服务驱动"的进程
    - 拉 Run/RunOnce 4 个 hive 的自启动条目,关联到候选清理项
    - 输出 6 个分类表 + 末尾的"候选清理 + 关联线索"详表(便于人工拍板加进画像)

.PARAMETER OutFile
  可选,把完整报告写入文件(便于事后回看)。默认仅控制台输出。

.NOTES
  本脚本不需要管理员。运行:
    powershell -NoProfile -ExecutionPolicy Bypass -File scripts\diagnostics\scan-processes.ps1
#>
[CmdletBinding()]
param(
  [string]$OutFile = '',
  [string]$ProfileDir = ''
)

$ErrorActionPreference = 'Continue'

# ---- 兜底路径 ----
if ([string]::IsNullOrEmpty($ProfileDir)) {
  $here = if ($PSScriptRoot) { $PSScriptRoot } elseif ($MyInvocation.MyCommand.Path) { Split-Path -Parent $MyInvocation.MyCommand.Path } else { (Get-Location).Path }
  $ProfileDir = Resolve-Path (Join-Path $here '..\..\profiles') -ErrorAction SilentlyContinue
}

# ============ 1. 加载画像档案指纹 ============
$profileHits = @{}  # processName(lower) -> profileId
$profileSvcs = @{}  # serviceName(lower) -> profileId
$profiles    = @()
if ($ProfileDir -and (Test-Path $ProfileDir)) {
  Get-ChildItem -LiteralPath $ProfileDir -Filter '*.json' -ErrorAction SilentlyContinue | ForEach-Object {
    try {
      $p = Get-Content -LiteralPath $_.FullName -Raw -Encoding UTF8 | ConvertFrom-Json
      $profiles += $p
      foreach ($n in @($p.fingerprints.process_names)) { if ($n) { $profileHits[$n.ToLower()] = $p.id } }
      foreach ($s in @($p.fingerprints.service_names)) { if ($s) { $profileSvcs[$s.ToLower()] = $p.id } }
    } catch { Write-Warning "解析画像失败: $($_.FullName): $($_.Exception.Message)" }
  }
}

# ============ 2. 系统进程白名单(高置信) ============
# Microsoft 自家进程, 一律保留。无需签名校验, 名字+路径就够。
$systemNames = @(
  # 内核/会话
  'System','Registry','System Idle Process','Secure System','smss.exe','csrss.exe','wininit.exe','winlogon.exe',
  'services.exe','lsass.exe','LsaIso.exe','NgcIso.exe','svchost.exe','fontdrvhost.exe','dwm.exe','dasHost.exe',
  # 用户态外壳
  'explorer.exe','sihost.exe','taskhostw.exe','ctfmon.exe','conhost.exe','RuntimeBroker.exe',
  'ApplicationFrameHost.exe','ShellExperienceHost.exe','ShellHost.exe','StartMenuExperienceHost.exe',
  'SearchHost.exe','SearchApp.exe','SearchIndexer.exe','SearchProtocolHost.exe','SearchFilterHost.exe',
  'dllhost.exe','LockApp.exe','UserOOBEBroker.exe','CompPkgSrv.exe','CrossDeviceResume.exe','AppActions.exe',
  'AggregatorHost.exe','TextInputHost.exe','TabTip.exe','TextInputServer.exe',
  # 音视频
  'audiodg.exe','spoolsv.exe',
  # UI 组件
  'Widgets.exe','WidgetService.exe','SystemSettings.exe',
  # Defender / 安全
  'NisSrv.exe','MsMpEng.exe','SecurityHealthService.exe','SecurityHealthSystray.exe','SgrmBroker.exe',
  'SgrmLpac.exe','smartscreen.exe','PhoneExperienceHost.exe',
  # GameInput / Xbox
  'GameInputRedistService.exe','GameInputSvc.exe','xgamehelper.exe','tcui-app.exe','XboxPcAppFT.exe',
  'gamingservices.exe','gamingservicesnet.exe','GamePanel.exe',
  # 驱动框架
  'WUDFHost.exe','wlanext.exe','PerfHost.exe','WmiPrvSE.exe','WmiApSrv.exe','dasHost.exe',
  # Hyper-V
  'vmcompute.exe','vmms.exe','vmwp.exe',
  # Edge 系统服务(微软自家)
  'elevation_service.exe','MicrosoftEdgeUpdate.exe',
  # 计算/低功耗管理
  'Memory Compression','backgroundTaskHost.exe','DataExchangeHost.exe',
  # 截图/系统应用
  'SnippingTool.exe','MoUsoCoreWorker.exe','usocoreworker.exe'
)
# 系统进程的额外正则(覆盖按版本带后缀的微软进程)
$systemNamePatterns = @(
  '^WmiPrvSE',           # WMI Provider
  '_2726c00f$',          # Win11 用户级 svchost 实例后缀
  '^Microsoft\.'         # UWP 类
)
function Test-SystemProcess {
  param($Name,$Path)
  if ($systemNames -contains $Name) { return $true }
  foreach ($p in $systemNamePatterns) { if ($Name -match $p) { return $true } }
  if ($Path -and ($Path -match '^C:\\Windows\\' -and $Path -notmatch '\\Temp\\' -and $Path -notmatch '\\Tasks\\')) { return $true }
  return $false
}

# ============ 3. 用户友好白名单(用户已知/认可的常用软件) ============
# 这是产品里"激进清理"会保留的进程。规则跑稳后,这部分会做成"可视化勾选"。
$userKnownGood = @(
  # 通讯
  'QQ.exe','QQEX.exe','QQBrowser.exe','WeChat.exe','WeChatAppEx.exe','Wemeet.exe','wemeetapp.exe','dingtalk.exe',
  'feishu.exe','lark.exe','TIM.exe','KOOK.exe','Discord.exe','Slack.exe','Telegram.exe','Element.exe',
  # 浏览器
  'msedge.exe','msedgewebview2.exe','chrome.exe','firefox.exe','brave.exe','vivaldi.exe','LibreWolf.exe','iexplore.exe',
  # 编辑/开发
  'Code.exe','code.exe','devenv.exe','idea64.exe','pycharm64.exe','goland64.exe','clion64.exe','rider64.exe','webstorm64.exe',
  'sublime_text.exe','notepad++.exe','obsidian.exe','typora.exe','HBuilderX.exe','Cursor.exe',
  'claude.exe','windsurf.exe','cowork-svc.exe','node.exe','bash.exe','git.exe',
  # 终端/工具
  'WindowsTerminal.exe','OpenConsole.exe','wsl.exe','wslhost.exe','wslservice.exe','powershell.exe','pwsh.exe','cmd.exe',
  'Everything.exe','ditto.exe','7zFM.exe','WinRAR.exe',
  # 媒体/办公
  'POTPLAYER.EXE','PotPlayerMini64.exe','vlc.exe','foobar2000.exe','musicbee.exe','Spotify.exe',
  'cloudmusic.exe','cloudmusic_reporter.exe',
  'EXCEL.EXE','WINWORD.EXE','POWERPNT.EXE','OUTLOOK.EXE','ONENOTE.EXE','OfficeClickToRun.exe','ai.exe',
  'MSOfficePLUSService.exe',
  # 游戏/平台
  'steam.exe','steamwebhelper.exe','steamservice.exe','EpicGamesLauncher.exe','UbisoftConnect.exe','RiotClientServices.exe',
  'GameViewer.exe','GameViewerService.exe','GameViewerServer.exe','GameViewerHealthd.exe','client.exe','wegame.exe',
  # NetEase UU 加速器(配合 GameViewer)
  'uu.exe','uu_ball.exe','uu_launcher.exe','uu_cloudsyn.exe',
  # 系统类工具(用户特意装的)
  'TranslucentTB.exe','Nexus.exe','WsxService.exe','AutoHotkey.exe','AutoHotkey64.exe','Rainmeter.exe',
  # 壁纸引擎
  'wallpaper64.exe','wallpaperservice32.exe','wallpaper32.exe','wallpaper_engine.exe',
  # 火绒(用户已表态保留)
  'HipsDaemon.exe','HipsMain.exe','HipsTray.exe','wsctrl.exe','wsctrlsvc.exe','sysdiag.exe',
  # FlClash(用户的代理)
  'FlClash.exe','FlClashCore.exe','FlClashHelperService.exe',
  # Radmin VPN
  'RvControlSvc.exe','RAdmin.exe',
  # 工具
  'crashpad_handler.exe'
)
# 用户应用的正则(覆盖一些带版本号/变形的进程名)
$userKnownPatterns = @(
  '^WeType',                  # 微信输入法(腾讯), 默认归用户应用; 不要可移到画像
  '^uu_',                     # NetEase UU 衍生进程
  '^PopBlock'                 # 第三方弹窗拦截(用户主动装)
)

# ============ 4. 硬件厂商工具(给硬件用的, 一般保留) ============
$hardwareTools = @(
  # NVIDIA
  'nvcontainer.exe','NVDisplay.Container.exe','nvidia share.exe','nvsphelper64.exe','NVIDIA Web Helper.exe','RtkAudUService64.exe',
  # AMD
  'AMDRSSrcExt.exe','AMDRSServ.exe','cncmd.exe','amdow.exe','RadeonSoftware.exe','Radeonsoftware.exe',
  'atieclxx.exe','atiesrxx.exe','AUEPLauncher.exe','AUEPDU.exe','amdfendrsr.exe','AmdPpkgSvc.exe',
  # Intel
  'igfxEM.exe','igfxHK.exe','igfxTray.exe','IntelCpHDCPSvc.exe',
  # ASUS / ROG
  'AsusCertService.exe','asComSvc.exe','atkexComSvc.exe','AcPowerNotification.exe','ArmouryCrate.exe',
  'AsusOptimization.exe','AsusSwitch.exe','GHelper.exe','GCC.exe',
  # 主板 (Gigabyte / 技嘉)
  'EasyTuneEngineService.exe','GBTECService.exe','GigabyteUpdateService.exe','GBT_DL_LIB.exe',
  # 外设
  'CorsairService.exe','CorsairDeviceControlService.exe','iCUE.exe','parfait_crash_handler.exe',
  'LogiOverlay.exe','LGHUB.exe','LGHUB Agent.exe','RGB Fusion.exe',
  # Corsair NGenuity
  'NGenuity2.exe','NGenuity2Helper.exe',
  # 显示器/机箱
  'JONSBO PC Monitor.exe','CPUMetricsServer.exe','SystemInfos.exe',
  # 音频(DTS / Realtek)
  'DtsHPXV2Apo4Service.exe','RtkBtManServ.exe','RtkAudUService.exe',
  # VR (PICO 4 等)
  'ps_service.exe','ps_service_launcher.exe','ps_server.exe',
  # 微软自带输入法
  'ChsIME.exe','PINYINUP.EXE',
  # 其他: 未识别的潜在硬件相关
  'RPMDaemon.exe'              # RGB/外设 daemon(推断)
)
# 硬件厂商进程名正则
$hardwareNamePatterns = @(
  '^amd',                     # amdow, amdfendrsr 等
  '^Amd',                     # AmdPpkgSvc
  '^AMD',                     # AMDRSServ
  '^Rtk',                     # Realtek
  '^Asus','^asus',
  '^ROG',
  '^GBT',                     # Gigabyte
  '^EasyTune',
  '^Corsair',
  '^iCUE',
  '^NGenuity',
  '^JONSBO',
  '^PICO','^pico','^ps_',
  '^DTS','^Dts','^dts',
  '^Intel','^igfx',
  '^NVIDIA','^nvidia','^nv'
)

# ============ 4b. 硬件/系统服务路径模式(用于反向归类没有进程名特征的进程) ============
# 进程本身没匹配到任何白名单, 但它跑的是一个明显硬件/系统厂商的服务, 也归对应类
$hardwareServicePathPatterns = @(
  '\\AMD\\','\\NVIDIA\\','\\Intel\\','\\Realtek\\','\\Asus\\','\\ASUSTeK',
  '\\Gigabyte\\','\\Corsair\\','\\JONSBO\\','\\PICO\\','\\Logitech\\','\\Razer\\',
  'Wallpaper Engine'
)
$userServicePathPatterns = @(
  '\\Steam\\','\\OfficeClickToRun','\\OfficePLUS\\','\\Tencent\\','\\Microsoft\\Edge\\',
  '\\Netease\\','\\Huorong\\','\\FlClash\\','\\Radmin','\\Wallpaper Engine\\',
  '\\Claude\\','\\Cowork'
)

# ============ 5. 拉数据 ============
$procs    = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue
$services = Get-CimInstance Win32_Service -ErrorAction SilentlyContinue
$drivers  = Get-CimInstance Win32_SystemDriver -ErrorAction SilentlyContinue
$svcByPid = @{}
foreach ($s in $services) { if ($s.ProcessId -gt 0) {
  if (-not $svcByPid.ContainsKey([int]$s.ProcessId)) { $svcByPid[[int]$s.ProcessId] = @() }
  $svcByPid[[int]$s.ProcessId] += "$($s.Name) [$($s.State)/$($s.StartMode)]"
}}

# 自启动项(Run/RunOnce)
$runHives = @(
  'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run',
  'HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce',
  'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run',
  'HKLM:\Software\Microsoft\Windows\CurrentVersion\RunOnce',
  'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run',
  'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce'
)
$runItems = @()
foreach ($h in $runHives) {
  if (Test-Path $h) {
    $props = (Get-Item -LiteralPath $h).Property
    foreach ($p in $props) {
      try {
        $v = (Get-ItemProperty -LiteralPath $h -Name $p).$p
        $runItems += [pscustomobject]@{ Hive=$h; Name=$p; Command=$v }
      } catch {}
    }
  }
}

# ============ 6. 分类 ============
$classified = New-Object System.Collections.Generic.List[object]
foreach ($p in $procs) {
  $name  = [string]$p.Name
  $exe   = [string]$p.ExecutablePath
  $pid_  = [int]$p.ProcessId
  $ppid  = [int]$p.ParentProcessId
  $cls   = 'unknown'
  $tags  = New-Object System.Collections.Generic.List[string]
  $svcs  = @()
  if ($svcByPid.ContainsKey($pid_)) { $svcs = $svcByPid[$pid_]; [void]$tags.Add('service-driven') }

  $profileId = $null
  if ($profileHits.ContainsKey($name.ToLower())) { $profileId = $profileHits[$name.ToLower()] }

  # 服务关联的 PathName(用于按服务路径反向归类)
  $svcPath = ''
  if ($svcs.Count -gt 0) {
    foreach ($s in $services) { if ([int]$s.ProcessId -eq $pid_) { $svcPath += ' ' + [string]$s.PathName } }
  }

  function _matchAny { param($s,$pats) foreach ($p in $pats) { if ($s -match $p) { return $true } } return $false }

  if ($profileId) {
    $cls = 'profile-known-rogue'
    [void]$tags.Add("PROFILE:$profileId")
  }
  elseif (Test-SystemProcess -Name $name -Path $exe) {
    $cls = 'system'
  }
  elseif ($hardwareTools -contains $name -or (_matchAny $name $hardwareNamePatterns)) {
    $cls = 'hardware-tool'
  }
  elseif ($userKnownGood -contains $name -or (_matchAny $name $userKnownPatterns)) {
    $cls = 'user-app-good'
  }
  elseif ($svcPath -and (_matchAny $svcPath $hardwareServicePathPatterns)) {
    $cls = 'hardware-tool'
    [void]$tags.Add('matched-by-service-path')
  }
  elseif ($svcPath -and (_matchAny $svcPath $userServicePathPatterns)) {
    $cls = 'user-app-good'
    [void]$tags.Add('matched-by-service-path')
  }
  elseif ($svcs.Count -gt 0) {
    # 由非系统/非已知服务驱动的进程: 真正的可疑保活
    $cls = 'service-keepalive-suspect'
  }
  elseif ($exe -and ($exe -match '\\ProgramData\\' -or $exe -match '\\AppData\\Roaming\\' -or $name -match 'Update|Updater|Helper|Daemon|Monitor|Maintenance|Background')) {
    $cls = 'name-pattern-suspect'
  }
  else {
    $cls = 'unknown'
  }

  # 关联自启动项
  $autoruns = @()
  if ($exe) {
    foreach ($r in $runItems) {
      if ($r.Command -and ($r.Command -match [regex]::Escape($name) -or ($exe -and $r.Command -match [regex]::Escape((Split-Path -Leaf $exe))))) {
        $autoruns += "$($r.Hive | Split-Path -Leaf)::$($r.Name)"
      }
    }
  }

  $classified.Add([pscustomobject]@{
    Class    = $cls
    Name     = $name
    PID      = $pid_
    PPID     = $ppid
    ExePath  = $exe
    Services = ($svcs -join '; ')
    Autoruns = ($autoruns -join '; ')
    Tags     = ($tags -join ',')
  })
}

# ============ 7. 输出 ============
$buf = New-Object System.Text.StringBuilder
function W([string]$s,[ConsoleColor]$c='Gray') { Write-Host $s -ForegroundColor $c; [void]$buf.AppendLine($s) }

W ("=" * 80) Cyan
W ("mingchuang / 进程分类扫描   {0:yyyy-MM-dd HH:mm:ss}" -f (Get-Date)) Cyan
W ("画像档案: {0} 份,指纹命中进程名 {1} 个" -f $profiles.Count, $profileHits.Count) DarkGray
W ("=" * 80) Cyan
W ""

$summary = $classified | Group-Object Class | Sort-Object Count -Descending
W "分类统计:" Yellow
foreach ($g in $summary) { W ("  {0,-30}  {1,5} 个" -f $g.Name, $g.Count) }
W ""

$classOrder = @(
  @{ key='profile-known-rogue';        title='已在画像中识别为流氓 (PROFILE 命中)';        color='Red' },
  @{ key='service-keepalive-suspect';  title='由非系统服务驱动 (可疑保活, 待人工拍板)';     color='Yellow' },
  @{ key='name-pattern-suspect';       title='命名/路径疑似保活 (Update/Helper/Maintenance/AppData/ProgramData)'; color='Yellow' },
  @{ key='user-app-good';              title='用户已知应用 (内置白名单)';                   color='Green' },
  @{ key='hardware-tool';              title='硬件厂商工具 (内置白名单)';                   color='Green' },
  @{ key='system';                     title='系统进程 (Microsoft)';                        color='DarkGray' },
  @{ key='unknown';                    title='未知 (规则未覆盖, 需要人工拍板)';             color='Magenta' }
)

foreach ($co in $classOrder) {
  $items = $classified | Where-Object Class -eq $co.key | Sort-Object Name,PID
  if (-not $items) { continue }
  W ("-" * 80) DarkGray
  W ("[{0}]  共 {1} 项" -f $co.title, $items.Count) $co.color
  W ("-" * 80) DarkGray
  foreach ($i in $items) {
    $line = "{0,-30} PID={1,-6} {2}" -f $i.Name, $i.PID, $i.ExePath
    W $line
    if ($i.Services) { W ("    服务: {0}" -f $i.Services) DarkGray }
    if ($i.Autoruns) { W ("    自启: {0}" -f $i.Autoruns) DarkGray }
    if ($i.Tags)     { W ("    标签: {0}" -f $i.Tags)     DarkGray }
  }
  W ""
}

# ============ 8. 候选清理详表(可疑保活 + 未知)聚焦输出 ============
W ("=" * 80) Cyan
W "候选清理项汇总 (建议人工拍板:加进画像 / 加进白名单 / 暂时忽略)" Cyan
W ("=" * 80) Cyan
$candidates = $classified | Where-Object { $_.Class -in 'profile-known-rogue','service-keepalive-suspect','name-pattern-suspect','unknown' } |
              Sort-Object Class,Name,PID
if ($candidates) {
  $candidates | ForEach-Object {
    W ("[{0}] {1}  PID={2}" -f $_.Class, $_.Name, $_.PID) Yellow
    W ("    路径: {0}" -f $_.ExePath)
    if ($_.Services) { W ("    服务: {0}" -f $_.Services) }
    if ($_.Autoruns) { W ("    自启: {0}" -f $_.Autoruns) }
    W ""
  }
} else { W "  (无候选项,所有进程都已分类)" Green }

if ($OutFile) {
  $buf.ToString() | Set-Content -LiteralPath $OutFile -Encoding UTF8
  W ("`n报告已保存: {0}" -f $OutFile) Cyan
}

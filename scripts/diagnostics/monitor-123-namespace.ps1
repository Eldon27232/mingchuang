#Requires -RunAsAdministrator
<#
.SYNOPSIS
  全自动溯源 123云盘 在"此电脑"命名空间伪文件夹的"自动复原"元凶。

.DESCRIPTION
  全程零交互:
    1) 挂注册表审核 SACL + 开 4663 成功审核;
    2) 同时监听 5 个可能位置(NameSpace 子键 + HideMyComputerIcons 等"软删除"标志位);
    3) 脚本自己 reg delete 目标键(确保删的就是 HKCU NameSpace 那个);
    4) 监控 180 秒, 30ms 高频轮询; 期间持续抓 4663 审核日志, 命中 CLSID 全部打印;
    5) 自动还原审核与 SACL, 不留痕。

  日志路径会在脚本启动时打印, 完整保存所有事件供事后分析。
#>
[CmdletBinding()]
param(
  [string]$Clsid    = '{D5BE1ADA-C1D3-4DF9-9317-95D61C28F6FA}',
  [string]$Match    = '123pan|123SyncCloud|123云盘|123Pan',
  [int]$PollMs      = 10,
  [int]$WatchSec    = 60,
  [switch]$Manual,
  [string]$LogDir   = ''
)

$ErrorActionPreference = 'Continue'

# $PSScriptRoot 在 [CmdletBinding()] 脚本的 param 默认期为空, 兜底
if ([string]::IsNullOrEmpty($LogDir)) {
  if ($PSScriptRoot) { $LogDir = $PSScriptRoot }
  elseif ($MyInvocation.MyCommand.Path) { $LogDir = Split-Path -Parent $MyInvocation.MyCommand.Path }
  else { $LogDir = (Get-Location).Path }
}

$REG_SUBCAT = '{0CCE921E-69AE-11D9-BED3-505054503030}'
$NS_REL     = 'Software\Microsoft\Windows\CurrentVersion\Explorer\MyComputer\NameSpace'
$NS_FULL    = "HKCU:\$NS_REL"
$TARGET     = "$NS_FULL\$Clsid"

# 额外监视的"软删除"标志位 —— UI 删除若不改 NameSpace, 通常会改这里
$EXTRA_WATCH = @(
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\HideMyComputerIcons",
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\NewStartPanel",
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\ClassicStartMenu",
  "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\HideMyComputerIcons",
  "HKCU:\Software\Classes\CLSID\$Clsid"
)

if (-not (Test-Path -LiteralPath $LogDir)) { New-Item -ItemType Directory -Path $LogDir -Force | Out-Null }
$logFile = Join-Path $LogDir ("123-namespace-monitor-{0:yyyyMMdd-HHmmss}.log" -f (Get-Date))

function Write-Log {
  param([string]$Msg, [ConsoleColor]$Color = 'Gray')
  $line = ("{0:HH:mm:ss.fff}  {1}" -f (Get-Date), $Msg)
  Write-Host $line -ForegroundColor $Color
  Add-Content -LiteralPath $logFile -Value $line -Encoding UTF8
}

function Enable-AuditPrivilege {
  $sig = @'
using System;
using System.Runtime.InteropServices;
public static class PrivHelper {
  [DllImport("advapi32.dll", SetLastError=true)]
  static extern bool OpenProcessToken(IntPtr h, uint acc, out IntPtr tok);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  [DllImport("advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
  static extern bool LookupPrivilegeValue(string host, string name, out long luid);
  [StructLayout(LayoutKind.Sequential)] struct TOKPRIV { public uint Count; public long Luid; public uint Attr; }
  [DllImport("advapi32.dll", SetLastError=true)]
  static extern bool AdjustTokenPrivileges(IntPtr tok, bool dis, ref TOKPRIV newst, uint len, IntPtr prev, IntPtr relen);
  public static bool Enable(string priv){
    IntPtr tok;
    if(!OpenProcessToken(GetCurrentProcess(), 0x28, out tok)) return false;
    long luid;
    if(!LookupPrivilegeValue(null, priv, out luid)) return false;
    TOKPRIV tp = new TOKPRIV(); tp.Count = 1; tp.Luid = luid; tp.Attr = 0x2;
    return AdjustTokenPrivileges(tok, false, ref tp, 0, IntPtr.Zero, IntPtr.Zero);
  }
}
'@
  try { if (-not ('PrivHelper' -as [type])) { Add-Type -TypeDefinition $sig } ; [void][PrivHelper]::Enable('SeSecurityPrivilege') } catch {}
}

function Snapshot-Targets {
  $out = New-Object System.Collections.Generic.List[string]
  $out.Add('  -- 进程 --')
  $procs = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
    ($_.ExecutablePath -and $_.ExecutablePath -match $Match) -or ($_.CommandLine -and $_.CommandLine -match $Match)
  }
  if ($procs) {
    foreach ($p in $procs) { $out.Add(("    PID={0} PPID={1} 启动={2} {3}" -f $p.ProcessId, $p.ParentProcessId, $p.CreationDate, $p.ExecutablePath)) }
  } else { $out.Add('    (无)') }

  $out.Add('  -- 服务 --')
  $svcs = Get-CimInstance Win32_Service -ErrorAction SilentlyContinue | Where-Object {
    ($_.PathName -and $_.PathName -match $Match) -or ($_.DisplayName -and $_.DisplayName -match $Match) -or ($_.Name -match $Match)
  }
  if ($svcs) {
    foreach ($s in $svcs) { $out.Add(("    {0} [{1}] 启动类型={2} PID={3} {4}" -f $s.Name, $s.State, $s.StartMode, $s.ProcessId, $s.PathName)) }
  } else { $out.Add('    (无)') }
  return ($out -join "`r`n")
}

# 拉 4663 审核事件, 命中 CLSID 字符串就打印(放宽过滤)
$seenRecords = New-Object System.Collections.Generic.HashSet[long]
function Collect-AuditEvents {
  param([datetime]$Since)
  try {
    $evts = Get-WinEvent -FilterHashtable @{ LogName = 'Security'; Id = 4663; StartTime = $Since } -ErrorAction SilentlyContinue
  } catch { return }
  if (-not $evts) { return }
  foreach ($e in $evts) {
    if (-not $seenRecords.Add([long]$e.RecordId)) { continue }
    $x = [xml]$e.ToXml()
    $d = @{}
    foreach ($n in $x.Event.EventData.Data) { $d[$n.Name] = $n.'#text' }
    $obj = [string]$d['ObjectName']
    $proc = [string]$d['ProcessName']
    # 命中 CLSID 字符串(去掉花括号匹配更宽), 或对象路径含 NameSpace
    $clsidStripped = $Clsid.Trim('{','}')
    $hitByPath = ($obj -match [regex]::Escape($clsidStripped) -or $obj -match 'MyComputer\\NameSpace' -or $obj -match 'HideMyComputerIcons' -or $obj -match 'Explorer')
    $hitByPid  = ($script:WatchPids -and ($script:WatchPids -contains [int]$d['ProcessId']))
    if ($hitByPath -or $hitByPid) {
      Write-Log ("【4663】写入进程: {0} (PID={1}) AccessMask={2}`r`n            对象: {3}" -f $proc, $d['ProcessId'], $d['AccessMask'], $obj) Yellow
    }
  }
}

# 监视点快照: 返回所有监视位置当前是否存在/或子键中是否含 CLSID 标记
function Get-WatchState {
  $st = [ordered]@{}
  $st['NS_TARGET'] = [bool](Test-Path -LiteralPath $TARGET)
  foreach ($p in $EXTRA_WATCH) {
    if (Test-Path -LiteralPath $p) {
      try {
        $vals = (Get-Item -LiteralPath $p -ErrorAction Stop).Property
        $hit = $null
        foreach ($v in $vals) {
          if ($v -match [regex]::Escape($Clsid)) {
            $data = (Get-ItemProperty -LiteralPath $p -Name $v).$v
            $hit = "$v=$data"
            break
          }
        }
        $st[$p] = if ($hit) { "EXISTS_HIT[$hit]" } else { "EXISTS" }
      } catch { $st[$p] = "EXISTS_ERR" }
    } else { $st[$p] = "MISSING" }
  }
  return $st
}

function Format-StateDelta {
  param($prev, $curr)
  $changes = @()
  foreach ($k in $curr.Keys) {
    $a = $prev[$k]; $b = $curr[$k]
    if ($a -ne $b) { $changes += "    $k : [$a] -> [$b]" }
  }
  return $changes
}

# ============ 主流程 ============
$auditKey = $null
$auditOn  = $false
$polWasOn = $false

try {
  Write-Log "日志文件: $logFile" Cyan
  Write-Log "目标 CLSID: $Clsid" Cyan
  Write-Log "主监视键: $TARGET" Cyan
  Write-Log "辅助监视位置数: $($EXTRA_WATCH.Count)" Cyan
  Write-Log "轮询: ${PollMs}ms  监控时长: ${WatchSec}s" Cyan
  Write-Log ""

  Enable-AuditPrivilege

  try {
    $cur = (auditpol /get /subcategory:"$REG_SUBCAT" /r 2>$null | ConvertFrom-Csv)
    $polWasOn = ($cur.'Inclusion Setting' -match 'Success')
    auditpol /set /subcategory:"$REG_SUBCAT" /success:enable | Out-Null
    Write-Log "已开启[对象访问-注册表]成功审核 (原状态: $($cur.'Inclusion Setting'))" DarkGray
  } catch { Write-Log "auditpol 设置失败: $($_.Exception.Message)" Red }

  try {
    $auditKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey(
      $NS_REL,
      [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree,
      [System.Security.AccessControl.RegistryRights]'ChangePermissions,ReadPermissions')
    $acl = $auditKey.GetAccessControl([System.Security.AccessControl.AccessControlSections]::Audit)
    $rule = New-Object System.Security.AccessControl.RegistryAuditRule(
      'Everyone',
      [System.Security.AccessControl.RegistryRights]'CreateSubKey,SetValue,Delete,WriteKey,EnumerateSubKeys',
      ([System.Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit'),
      [System.Security.AccessControl.PropagationFlags]::None,
      [System.Security.AccessControl.AuditFlags]::Success)
    $acl.AddAuditRule($rule)
    $auditKey.SetAccessControl($acl)
    $auditOn = $true
    Write-Log "已对 NameSpace 父键挂载 SACL (含子键继承)" Green
  } catch {
    Write-Log "SACL 挂载失败,降级为【仅轮询】: $($_.Exception.Message)" Red
  }

  Write-Log ""
  Write-Log "===== 基线快照 =====" Cyan
  Write-Log (Snapshot-Targets)
  Write-Log ""
  $state0 = Get-WatchState
  Write-Log "===== 监视位置基线 =====" Cyan
  foreach ($k in $state0.Keys) { Write-Log ("    {0} : {1}" -f $k, $state0[$k]) }
  Write-Log ""

  if ($Manual) {
    Write-Log "▶ 手动模式: 请现在去『此电脑』里【右键 123云盘 → 删除】" Magenta
    Write-Log "    脚本不会自动删, 只被动监听 s, 期间持续打印审核命中。" White
  } elseif (-not $state0['NS_TARGET']) {
    Write-Log "⚠ 目标键已不存在, 直接进入监控等待复原..." Yellow
  } else {
    Write-Log "▶ 脚本自动 reg delete 目标键..." Cyan
    $delOut = & reg delete "HKCU\$NS_REL\$Clsid" /f 2>&1
    Write-Log ("    reg delete 结果: " + ($delOut -join ' | ')) DarkGray
    $existsAfterDel = [bool](Test-Path -LiteralPath $TARGET)
    Write-Log "    删除后立即 Test-Path: $existsAfterDel" $(if($existsAfterDel){'Red'}else{'Green'})
  }

  # 收集 123 服务的当前 PID 列表, 用于审核命中放宽
  $script:WatchPids = @()
  Get-CimInstance Win32_Service -ErrorAction SilentlyContinue | Where-Object {
    ($_.PathName -and $_.PathName -match $Match) -or ($_.Name -match $Match)
  } | ForEach-Object { if ($_.ProcessId -gt 0) { $script:WatchPids += [int]$_.ProcessId } }
  Write-Log ("    监视 PID 列表: " + ($script:WatchPids -join ', ')) DarkGray

  Write-Log ""
  Write-Log "===== 进入监控期 ${WatchSec}s =====" Cyan
  $prevState = Get-WatchState
  $lastScan  = (Get-Date).AddSeconds(-5)
  $tStart    = Get-Date
  $tDeadline = $tStart.AddSeconds($WatchSec)
  $restored  = $false
  $deltaCount = 0

  while ((Get-Date) -lt $tDeadline) {
    $cur = Get-WatchState
    $changes = Format-StateDelta -prev $prevState -curr $cur
    if ($changes.Count -gt 0) {
      $deltaCount++
      Write-Log "★ 监视位置变化 ($deltaCount):" Green
      foreach ($c in $changes) { Write-Log $c Green }
      if (-not $restored -and $cur['NS_TARGET'] -and -not $prevState['NS_TARGET']) {
        $restored = $true
        Write-Log "    >>> NS_TARGET 已被复原 <<<" Magenta
        Write-Log ("    复原瞬间快照:`r`n" + (Snapshot-Targets))
      }
      $prevState = $cur
    }

    if ($auditOn) { Collect-AuditEvents -Since $lastScan }
    $lastScan = (Get-Date).AddSeconds(-2)

    Start-Sleep -Milliseconds $PollMs
  }

  Write-Log ""
  Write-Log "===== 监控结束 =====" Cyan
  Write-Log ("总监视位置变化次数: $deltaCount  目标键被复原: $restored")
  Write-Log "===== 最终状态 ====="
  $endState = Get-WatchState
  foreach ($k in $endState.Keys) { Write-Log ("    {0} : {1}" -f $k, $endState[$k]) }
}
finally {
  Write-Log ""
  Write-Log "正在还原审核设置..." DarkGray
  if ($auditOn -and $auditKey) {
    try {
      $acl = $auditKey.GetAccessControl([System.Security.AccessControl.AccessControlSections]::Audit)
      [void]$acl.PurgeAuditRules([System.Security.Principal.NTAccount]'Everyone')
      $auditKey.SetAccessControl($acl)
    } catch { Write-Log "移除 SACL 失败: $($_.Exception.Message)" Red }
  }
  if ($auditKey) { $auditKey.Close() }
  if (-not $polWasOn) { auditpol /set /subcategory:"$REG_SUBCAT" /success:disable | Out-Null }
  Write-Log "已还原。日志: $logFile" Cyan
}

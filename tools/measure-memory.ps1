<#
.SYNOPSIS
    Auler 프로세스와 모든 하위 프로세스의 메모리 합계를 기록한다.

.DESCRIPTION
    기획서의 측정 지표(앱과 자식 프로세스의 Private Bytes 합계)를 시점별로 잰다.
    -Exe로 앱을 실행해 측정하거나, -ProcessId로 이미 실행 중인 앱에 붙는다.
    결과는 화면에 표로 보여주고 tools/out/memory.csv에 한 줄씩 쌓는다.

.EXAMPLE
    ./tools/measure-memory.ps1 -Exe target/release/auler.exe -Label idle

.EXAMPLE
    ./tools/measure-memory.ps1 -Exe target/release/auler.exe -Label idle-software -Env @{ SLINT_BACKEND = 'winit-software' }

.EXAMPLE
    ./tools/measure-memory.ps1 -ProcessId 12345 -Label after-scroll
#>
[CmdletBinding(DefaultParameterSetName = 'Launch')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Launch')]
    [string]$Exe,

    [Parameter(ParameterSetName = 'Launch')]
    [string[]]$ArgumentList = @(),

    # 앱에만 적용할 환경 변수 (예: SLINT_BACKEND)
    [Parameter(ParameterSetName = 'Launch')]
    [hashtable]$Env = @{},

    # 실행 후 첫 측정까지 기다리는 시간
    [Parameter(ParameterSetName = 'Launch')]
    [int]$WarmupSeconds = 10,

    # 측정 후 앱을 종료하지 않는다
    [Parameter(ParameterSetName = 'Launch')]
    [switch]$KeepRunning,

    [Parameter(Mandatory, ParameterSetName = 'Attach')]
    [int]$ProcessId,

    [string]$Label = 'sample',
    [int]$Samples = 5,
    [int]$IntervalMs = 1000,
    [string]$OutCsv = (Join-Path $PSScriptRoot 'out/memory.csv')
)

$ErrorActionPreference = 'Stop'

function Get-ProcessTreeId {
    param([int]$RootId)
    $children = @{}
    foreach ($p in Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId) {
        $parent = [int]$p.ParentProcessId
        if (-not $children.ContainsKey($parent)) {
            $children[$parent] = [System.Collections.Generic.List[int]]::new()
        }
        $children[$parent].Add([int]$p.ProcessId)
    }
    $seen = [System.Collections.Generic.HashSet[int]]::new()
    $queue = [System.Collections.Generic.Queue[int]]::new()
    $queue.Enqueue($RootId)
    while ($queue.Count -gt 0) {
        $id = $queue.Dequeue()
        if (-not $seen.Add($id)) { continue }   # PID 재사용으로 생기는 순환 방지
        $id
        if ($children.ContainsKey($id)) {
            foreach ($child in $children[$id]) { $queue.Enqueue($child) }
        }
    }
}

function Measure-ProcessTree {
    param([int]$RootId)
    foreach ($id in Get-ProcessTreeId -RootId $RootId) {
        $p = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($p) {
            [pscustomobject]@{
                Id           = $p.Id
                Name         = $p.ProcessName
                PrivateBytes = $p.PrivateMemorySize64
                WorkingSet   = $p.WorkingSet64
            }
        }
    }
}

function ConvertTo-MB { param([double]$Bytes) [math]::Round($Bytes / 1MB, 1) }

$launched = $null
if ($PSCmdlet.ParameterSetName -eq 'Launch') {
    $exePath = (Resolve-Path $Exe).Path
    $saved = @{}
    foreach ($key in $Env.Keys) {
        $saved[$key] = [Environment]::GetEnvironmentVariable($key)
        [Environment]::SetEnvironmentVariable($key, [string]$Env[$key])
    }
    try {
        $startArgs = @{ FilePath = $exePath; PassThru = $true }
        if ($ArgumentList.Count -gt 0) { $startArgs.ArgumentList = $ArgumentList }
        $launched = Start-Process @startArgs
    }
    finally {
        foreach ($key in $saved.Keys) { [Environment]::SetEnvironmentVariable($key, $saved[$key]) }
    }
    $rootId = $launched.Id
    Write-Host "실행: $exePath (PID $rootId). $WarmupSeconds초 뒤 측정합니다."
    Start-Sleep -Seconds $WarmupSeconds
}
else {
    $rootId = $ProcessId
}

$results = @()
$lastRows = @()
for ($i = 1; $i -le $Samples; $i++) {
    $rows = @(Measure-ProcessTree -RootId $rootId)
    if ($rows.Count -eq 0) { throw "PID $rootId 프로세스가 실행 중이 아닙니다." }
    $lastRows = $rows
    $results += [pscustomobject]@{
        Sample       = $i
        Processes    = $rows.Count
        PrivateBytes = ($rows | Measure-Object PrivateBytes -Sum).Sum
        WorkingSet   = ($rows | Measure-Object WorkingSet -Sum).Sum
    }
    if ($i -lt $Samples) { Start-Sleep -Milliseconds $IntervalMs }
}

Write-Host "`n마지막 측정의 프로세스별 값"
$lastRows | Select-Object Id, Name,
    @{ n = 'PrivateMB'; e = { ConvertTo-MB $_.PrivateBytes } },
    @{ n = 'WorkingSetMB'; e = { ConvertTo-MB $_.WorkingSet } } |
    Format-Table -AutoSize | Out-Host

$private = $results | Measure-Object PrivateBytes -Average -Maximum -Minimum
$working = $results | Measure-Object WorkingSet -Average -Maximum
$summary = [pscustomobject]@{
    Timestamp       = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
    Label           = $Label
    Process         = (Get-Process -Id $rootId).ProcessName
    Processes       = $results[-1].Processes
    PrivateMB_Avg   = ConvertTo-MB $private.Average
    PrivateMB_Min   = ConvertTo-MB $private.Minimum
    PrivateMB_Max   = ConvertTo-MB $private.Maximum
    WorkingSetMB_Avg = ConvertTo-MB $working.Average
    WorkingSetMB_Max = ConvertTo-MB $working.Maximum
    Samples         = $Samples
    Env             = ($Env.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ';'
}

$outDir = Split-Path $OutCsv -Parent
if (-not (Test-Path $outDir)) { New-Item -ItemType Directory -Path $outDir | Out-Null }
$csvArgs = @{ Path = $OutCsv; Append = $true; Encoding = 'utf8' }
# Windows PowerShell 5.1만 형식 정보 줄을 붙인다. PowerShell 7에서는 이 옵션이 없어도 된다.
if ($PSVersionTable.PSVersion.Major -lt 6) { $csvArgs.NoTypeInformation = $true }
$summary | Export-Csv @csvArgs

if ($launched -and -not $KeepRunning) {
    $tree = @(Get-ProcessTreeId -RootId $rootId)
    [array]::Reverse($tree)
    foreach ($id in $tree) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
}

$summary

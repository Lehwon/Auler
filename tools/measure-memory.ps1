<#
.SYNOPSIS
    Auler 프로세스와 모든 하위 프로세스의 메모리 합계를 기록한다.

.DESCRIPTION
    측정 지표(앱과 자식 프로세스의 Private Bytes 합계)를 시점별로 잰다.
    -Exe로 앱을 실행해 측정하거나, -ProcessId로 이미 실행 중인 앱에 붙는다.
    결과는 화면에 표로 보여주고 tools/out/memory.csv에 한 줄씩 쌓는다.
    -Exe로 실행한 앱은 측정이 끝나거나 오류·중단(Ctrl+C)이 나도 종료한다(-KeepRunning 제외).

.EXAMPLE
    ./tools/measure-memory.ps1 -Exe target/release/auler.exe -Label idle

.EXAMPLE
    ./tools/measure-memory.ps1 -Exe target/release/auler.exe -Label data -Env @{ AULER_PAGE = '1' }

.EXAMPLE
    ./tools/measure-memory.ps1 -ProcessId 12345 -Label after-scroll
#>
[CmdletBinding(DefaultParameterSetName = 'Launch')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Launch')]
    [string]$Exe,

    [Parameter(ParameterSetName = 'Launch')]
    [string[]]$ArgumentList = @(),

    # 앱에만 적용할 환경 변수 (예: AULER_PAGE)
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
Import-Module (Join-Path $PSScriptRoot 'AulerMeasure.psm1') -Force

$launched = $null
try {
    if ($PSCmdlet.ParameterSetName -eq 'Launch') {
        $launched = Start-MeasuredProcess -Exe $Exe -ArgumentList $ArgumentList -Env $Env
        $rootId = $launched.Id
        $processName = [System.IO.Path]::GetFileNameWithoutExtension($Exe)
        Write-Host "실행: $Exe (PID $rootId). $($WarmupSeconds)초 뒤 측정합니다."
        Start-Sleep -Seconds $WarmupSeconds
    }
    else {
        $rootId = $ProcessId
        $processName = (Get-Process -Id $ProcessId).ProcessName
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
        Timestamp        = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
        Label            = $Label
        Process          = $processName
        Processes        = $results[-1].Processes
        PrivateMB_Avg    = ConvertTo-MB $private.Average
        PrivateMB_Min    = ConvertTo-MB $private.Minimum
        PrivateMB_Max    = ConvertTo-MB $private.Maximum
        WorkingSetMB_Avg = ConvertTo-MB $working.Average
        WorkingSetMB_Max = ConvertTo-MB $working.Maximum
        Samples          = $Samples
        Env              = ($Env.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ';'
    }
    Add-CsvRow -Row $summary -Path $OutCsv
    $summary
}
finally {
    if ($launched -and -not $KeepRunning) { Stop-ProcessTree -Process $launched }
}

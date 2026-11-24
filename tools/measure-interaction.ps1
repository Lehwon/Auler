<#
.SYNOPSIS
    스크롤·끌기 중 CPU, 프레임 수, 메모리 피크를 잰다 (계획 0.4, 명세서 8장).

.DESCRIPTION
    앱을 AULER_BENCH 모드로 실행한다. 앱은 3초 준비 후 스스로 입력 이벤트를 넣고
    (실제 마우스는 움직이지 않음), 끝나면 요약 한 줄을 표준 오류에 쓰고 종료한다.
    SLINT_DEBUG_PERFORMANCE로 초당 렌더링 프레임 수도 함께 받는다.
    결과는 화면에 표로 보여주고 tools/out/interaction.csv에 쌓는다.

.EXAMPLE
    ./tools/measure-interaction.ps1 -Exe target/release/auler.exe

.EXAMPLE
    ./tools/measure-interaction.ps1 -Scenario drag -Seconds 10 -Repeat 3
#>
param(
    [string]$Exe = (Join-Path $PSScriptRoot '../target/release/auler.exe'),
    [ValidateSet('idle', 'scroll', 'drag')]
    [string[]]$Scenario = @('idle', 'scroll', 'drag'),
    [int]$Seconds = 5,
    [int]$Repeat = 1,
    [int]$TimeoutSeconds = 60,
    [string]$OutCsv = (Join-Path $PSScriptRoot 'out/interaction.csv')
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'AulerMeasure.psm1') -Force

$outDir = Join-Path $PSScriptRoot 'out'
if (-not (Test-Path $outDir)) { New-Item -ItemType Directory -Path $outDir | Out-Null }

$results = foreach ($name in $Scenario) {
    foreach ($run in 1..$Repeat) {
        $stderr = Join-Path $outDir "bench-$name-$run.err"
        $stdout = Join-Path $outDir "bench-$name-$run.out"
        Remove-Item $stderr, $stdout -ErrorAction SilentlyContinue

        $p = Start-MeasuredProcess -Exe $Exe -StdOut $stdout -StdErr $stderr -Env @{
            AULER_BENCH             = $name
            AULER_BENCH_SECONDS     = "$Seconds"
            SLINT_DEBUG_PERFORMANCE = 'refresh_lazy,console'
        }
        try {
            if (-not $p.WaitForExit($TimeoutSeconds * 1000)) {
                throw "$name 벤치마크가 $TimeoutSeconds초 안에 끝나지 않았습니다."
            }
        }
        finally {
            Stop-ProcessTree -Process $p
        }

        $text = (Get-Content $stderr -Raw -ErrorAction SilentlyContinue) + (Get-Content $stdout -Raw -ErrorAction SilentlyContinue)
        $line = ($text -split "`r?`n") | Where-Object { $_ -like 'AULER_BENCH *' } | Select-Object -Last 1
        if (-not $line) { throw "$name 결과 줄이 없습니다. $stderr 를 확인하세요." }
        $fields = @{}
        foreach ($pair in ($line -replace '^AULER_BENCH ', '') -split ' ') {
            $k, $v = $pair -split '=', 2
            $fields[$k] = $v
        }

        # 초당 프레임: 입력이 들어간 초(10 이상)만 평균. 준비·정리 구간의 0~2 프레임은 뺀다.
        # 첫 줄은 앱 시작 직후 1초(첫 화면 그리기)라서 뺀다.
        $fps = @([regex]::Matches($text, 'average frames per second: (\d+)') | ForEach-Object { [int]$_.Groups[1].Value } | Select-Object -Skip 1)
        $active = @($fps | Where-Object { $_ -ge 10 })
        $row = [pscustomobject]@{
            Timestamp      = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
            Scenario       = $name
            Run            = $run
            Seconds        = [double]$fields.seconds
            CpuCorePct     = [double]$fields.cpu_core_pct
            FpsAvg         = if ($active.Count) { [math]::Round(($active | Measure-Object -Average).Average, 1) } else { 0 }
            FpsMin         = if ($active.Count) { ($active | Measure-Object -Minimum).Minimum } else { 0 }
            PrivateBeforeMB = [double]$fields.private_before_mb
            PrivatePeakMB  = [double]$fields.private_peak_mb
            PrivateAfterMB = [double]$fields.private_after_mb
            Scale          = $fields.scale
            Content        = $fields.content
        }
        Add-CsvRow -Row $row -Path $OutCsv
        $row
    }
}

$results | Format-Table Scenario, Run, Seconds, CpuCorePct, FpsAvg, FpsMin, PrivateBeforeMB, PrivatePeakMB, PrivateAfterMB, Scale, Content -AutoSize

<#
.SYNOPSIS
    Compare the release PDF example with and without the embedded Typst renderer.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$out = Join-Path $root 'target/tmp/pdf-spike'
New-Item -ItemType Directory -Force -Path $out | Out-Null

Push-Location $root
try {
    foreach ($variant in @('baseline', 'with-pdf')) {
        $features = @()
        if ($variant -eq 'with-pdf') { $features = @('--features', 'pdf') }
        cargo build --locked --release -p auler-io --example pdf-spike @features
        if ($LASTEXITCODE -ne 0) { throw "Build failed for $variant" }
        $exe = Join-Path $out "$variant.exe"
        Copy-Item target/release/examples/pdf-spike.exe $exe -Force

        $log = Join-Path $out "$variant.log"
        $errors = Join-Path $out "$variant.err"
        $process = Start-Process -FilePath $exe -ArgumentList "`"$out`"" -PassThru `
            -WindowStyle Hidden -RedirectStandardOutput $log -RedirectStandardError $errors
        $clock = [System.Diagnostics.Stopwatch]::StartNew()
        $samples = [System.Collections.Generic.List[object]]::new()
        $peakCommit = 0
        while (-not $process.HasExited) {
            $process.Refresh()
            try {
                $peakCommit = [Math]::Max($peakCommit, $process.PeakPagedMemorySize64)
                $samples.Add([pscustomobject]@{
                    ElapsedMs = $clock.ElapsedMilliseconds
                    PrivateBytes = $process.PrivateMemorySize64
                    WorkingSetBytes = $process.WorkingSet64
                })
            } catch [System.InvalidOperationException] {
                if (-not $process.HasExited) { throw }
            }
            Start-Sleep -Milliseconds 10
        }
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) { throw (Get-Content -Raw $errors) }
        $samples | Export-Csv -Path (Join-Path $out "$variant.csv") -NoTypeInformation

        $before = @($samples | Where-Object { $_.ElapsedMs -ge 400 -and $_.ElapsedMs -lt 900 })
        $after = @($samples | Where-Object { $_.ElapsedMs -ge ($clock.ElapsedMilliseconds - 900) })
        if ($before.Count -eq 0 -or $after.Count -eq 0) { throw 'Missing memory samples' }
        $summary = [pscustomobject]@{
            Variant = $variant
            ExeBytes = (Get-Item $exe).Length
            PrivateBeforeMiB = [Math]::Round(($before | Measure-Object PrivateBytes -Average).Average / 1MB, 2)
            PrivatePeakSampleMiB = [Math]::Round(($samples | Measure-Object PrivateBytes -Maximum).Maximum / 1MB, 2)
            PrivateAfterMiB = [Math]::Round(($after | Measure-Object PrivateBytes -Average).Average / 1MB, 2)
            WindowsPeakCommitMiB = [Math]::Round($peakCommit / 1MB, 2)
            Run = (Get-Content $log | Select-Object -Last 1)
        }
        $summary | ConvertTo-Json | Set-Content -Path (Join-Path $out "$variant.json")
        $summary
    }
}
finally {
    Pop-Location
}

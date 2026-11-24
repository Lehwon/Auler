# 측정 스크립트(measure-memory.ps1, measure-interaction.ps1) 공용 함수

function Get-ProcessTreeId {
    <#
    .SYNOPSIS
        루트 프로세스와 모든 하위 프로세스의 ID.
    .DESCRIPTION
        PID는 재사용되므로, 부모 PID만 보면 끝난 프로세스의 PID를 물려받은 남의 프로세스를
        자식으로 잘못 셀 수 있다. 자식은 부모보다 나중에 생성된 프로세스만 인정한다.
    #>
    param([Parameter(Mandatory)][int]$RootId)

    $byId = @{}
    $children = @{}
    foreach ($p in Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, CreationDate) {
        $id = [int]$p.ProcessId
        $byId[$id] = $p
        $parent = [int]$p.ParentProcessId
        if (-not $children.ContainsKey($parent)) {
            $children[$parent] = [System.Collections.Generic.List[int]]::new()
        }
        $children[$parent].Add($id)
    }
    if (-not $byId.ContainsKey($RootId)) { return }

    $seen = [System.Collections.Generic.HashSet[int]]::new()
    $queue = [System.Collections.Generic.Queue[int]]::new()
    $queue.Enqueue($RootId)
    while ($queue.Count -gt 0) {
        $id = $queue.Dequeue()
        if (-not $seen.Add($id)) { continue }
        $id
        if (-not $children.ContainsKey($id)) { continue }
        $parentCreated = $byId[$id].CreationDate
        foreach ($child in $children[$id]) {
            $childCreated = $byId[$child].CreationDate
            if ($parentCreated -and $childCreated -and $childCreated -lt $parentCreated) { continue }
            $queue.Enqueue($child)
        }
    }
}

function Measure-ProcessTree {
    <# 프로세스 트리의 프로세스별 Private Bytes, Working Set, CPU 시간 #>
    param([Parameter(Mandatory)][int]$RootId)
    foreach ($id in Get-ProcessTreeId -RootId $RootId) {
        $p = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($p) {
            [pscustomobject]@{
                Id           = $p.Id
                Name         = $p.ProcessName
                PrivateBytes = $p.PrivateMemorySize64
                WorkingSet   = $p.WorkingSet64
                CpuSeconds   = $p.TotalProcessorTime.TotalSeconds
            }
        }
    }
}

function Stop-ProcessTree {
    <# 실행한 프로세스와 하위 프로세스를 모두 끝낸다. 이미 끝났으면 아무것도 하지 않는다. #>
    param([Parameter(Mandatory)][System.Diagnostics.Process]$Process)
    if ($Process.HasExited) { return }
    $ids = @(Get-ProcessTreeId -RootId $Process.Id)
    try {
        $Process.Kill($true)   # .NET Core 3+ (PowerShell 7): 트리 전체 종료
    }
    catch {
        [array]::Reverse($ids)
        foreach ($id in $ids) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
    }
}

function Start-MeasuredProcess {
    <# 지정한 환경 변수를 이 프로세스에만 적용해 실행한다. 표준 출력·오류를 파일로 받을 수 있다. #>
    param(
        [Parameter(Mandatory)][string]$Exe,
        [string[]]$ArgumentList = @(),
        [hashtable]$Env = @{},
        [string]$StdOut,
        [string]$StdErr
    )
    $exePath = (Resolve-Path $Exe).Path
    $saved = @{}
    foreach ($key in $Env.Keys) {
        $saved[$key] = [Environment]::GetEnvironmentVariable($key)
        [Environment]::SetEnvironmentVariable($key, [string]$Env[$key])
    }
    try {
        $startArgs = @{ FilePath = $exePath; PassThru = $true }
        if ($ArgumentList.Count -gt 0) { $startArgs.ArgumentList = $ArgumentList }
        if ($StdOut) { $startArgs.RedirectStandardOutput = $StdOut }
        if ($StdErr) { $startArgs.RedirectStandardError = $StdErr }
        Start-Process @startArgs
    }
    finally {
        foreach ($key in $saved.Keys) { [Environment]::SetEnvironmentVariable($key, $saved[$key]) }
    }
}

function ConvertTo-MB { param([double]$Bytes) [math]::Round($Bytes / 1MB, 1) }

function Add-CsvRow {
    param([Parameter(Mandatory)]$Row, [Parameter(Mandatory)][string]$Path)
    $dir = Split-Path $Path -Parent
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }
    $csvArgs = @{ Path = $Path; Append = $true; Encoding = 'utf8' }
    # Windows PowerShell 5.1만 형식 정보 줄을 붙인다.
    if ($PSVersionTable.PSVersion.Major -lt 6) { $csvArgs.NoTypeInformation = $true }
    $Row | Export-Csv @csvArgs
}

Export-ModuleMember -Function Get-ProcessTreeId, Measure-ProcessTree, Stop-ProcessTree, Start-MeasuredProcess, ConvertTo-MB, Add-CsvRow

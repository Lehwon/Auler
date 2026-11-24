<#
.SYNOPSIS
    배포 실행 파일에 들어가는 오픈소스 구성 요소 목록(THIRD-PARTY.md)을 만든다.

.DESCRIPTION
    Windows 대상의 일반 의존성(빌드·개발 전용, 절차적 매크로 제외)을 cargo tree로 모아
    이름, 버전, 라이선스, 저장소를 표로 쓴다. CI가 이 파일이 최신인지 검사한다.

.EXAMPLE
    ./tools/update-third-party.ps1
#>
param(
    [string]$Out = (Join-Path $PSScriptRoot '../THIRD-PARTY.md')
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent

Push-Location $repoRoot
try {
    $lines = cargo tree -p auler --locked -e normal,no-proc-macro --target x86_64-pc-windows-msvc `
        --prefix none --no-dedupe --format '{p}|{l}|{r}'
    if ($LASTEXITCODE -ne 0) { throw 'cargo tree 실패' }
}
finally {
    Pop-Location
}

$rows = $lines |
    Where-Object { $_ -and $_ -notmatch '^auler v' } |
    ForEach-Object {
        $name, $license, $repo = $_ -split '\|', 3
        $parts = ($name.Trim() -split ' ')
        [pscustomobject]@{
            Name       = $parts[0]
            Version    = $parts[1].TrimStart('v')
            License    = if ($license) { $license.Trim() } else { '(확인 필요)' }
            Repository = if ($repo) { $repo.Trim() } else { '' }
        }
    } |
    Sort-Object Name, Version -Unique

$sb = [System.Text.StringBuilder]::new()
[void]$sb.AppendLine('# 오픈소스 구성 요소')
[void]$sb.AppendLine()
[void]$sb.AppendLine('Auler 실행 파일(Windows)에 들어가는 오픈소스 구성 요소와 라이선스다.')
[void]$sb.AppendLine('`tools/update-third-party.ps1`로 만들며 직접 고치지 않는다. Auler 자체는 [MIT](LICENSE)다.')
[void]$sb.AppendLine()
[void]$sb.AppendLine('Slint는 GPL-3.0, Slint Royalty-free 2.0, Slint Software 3.0 중 하나로 쓸 수 있으며, Auler는 Royalty-free 2.0을 따른다. 앱의 정보 화면에 `AboutSlint`를 표시한다.')
[void]$sb.AppendLine()
[void]$sb.AppendLine("구성 요소 $(@($rows).Count)개")
[void]$sb.AppendLine()
[void]$sb.AppendLine('| 이름 | 버전 | 라이선스 | 저장소 |')
[void]$sb.AppendLine('|---|---|---|---|')
foreach ($r in $rows) {
    $repo = if ($r.Repository) { "<$($r.Repository)>" } else { '' }
    [void]$sb.AppendLine("| $($r.Name) | $($r.Version) | $($r.License) | $repo |")
}

$content = $sb.ToString().Replace("`r`n", "`n")
[System.IO.File]::WriteAllText((Resolve-Path -LiteralPath (Split-Path $Out -Parent)).Path + '/' + (Split-Path $Out -Leaf), $content, [System.Text.UTF8Encoding]::new($false))
Write-Host "$Out ($(@($rows).Count)개)"

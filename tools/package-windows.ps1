[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$packager = Join-Path $root 'target/tools/cargo-packager/bin/cargo-packager.exe'
if (-not (Test-Path -LiteralPath $packager -PathType Leaf)) {
    throw 'Install cargo-packager 0.11.8 as described in docs/install.md.'
}

$target = 'x86_64-pc-windows-msvc'
$output = Join-Path $root 'target/windows-installer'
$oldTargetDir = $env:CARGO_TARGET_DIR
$oldRustFlags = $env:CARGO_ENCODED_RUSTFLAGS

Push-Location -LiteralPath $root
try {
    $env:CARGO_TARGET_DIR = Join-Path $root 'target/windows-release'
    $env:CARGO_ENCODED_RUSTFLAGS = '-C' + [char]31 + 'target-feature=+crt-static'
    cargo build --release --locked --package auler --target $target
    if ($LASTEXITCODE -ne 0) { throw 'Auler release build failed.' }

    $metadataJson = cargo metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Could not read the Auler package version.' }
    $version = (($metadataJson | ConvertFrom-Json).packages | Where-Object name -eq 'auler').version
    & $packager --release --packages auler --target $target --out-dir $output
    if ($LASTEXITCODE -ne 0) { throw 'NSIS packaging failed.' }

    $installer = Get-Item -LiteralPath (Join-Path $output "auler_${version}_x64-setup.exe")
    $hash = (Get-FileHash -LiteralPath $installer.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath ($installer.FullName + '.sha256') -Encoding ascii -Value "$hash  $($installer.Name)"
    $installer
}
finally {
    $env:CARGO_TARGET_DIR = $oldTargetDir
    $env:CARGO_ENCODED_RUSTFLAGS = $oldRustFlags
    Pop-Location
}

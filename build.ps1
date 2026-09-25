param([switch]$TestOnly)
$ErrorActionPreference='Stop'
Push-Location $PSScriptRoot
try {
    $cargo=Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
    & $cargo test --workspace --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed' }
    if ($TestOnly) { return }
    & $cargo build --release --workspace --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed' }
    & (Join-Path $PSScriptRoot 'set-icon.ps1') -Executable (Join-Path $PSScriptRoot 'target\release\deathloop-mod-manager.exe')
    $dest=Join-Path $PSScriptRoot 'dist\DeathloopModManager-0.4.1'
    New-Item -ItemType Directory -Path $dest -Force | Out-Null
    foreach($source in @('target\release\deathloop-mod-manager.exe','target\release\dlmod_runtime.dll','README.md')) {
        $target=Join-Path $dest (Split-Path -Leaf $source)
        if ((Test-Path -LiteralPath $target) -and (Get-FileHash -LiteralPath $source).Hash -eq (Get-FileHash -LiteralPath $target).Hash) { continue }
        Copy-Item -LiteralPath $source -Destination $target -Force
    }
    # Preserve users' existing profile/mod files when rebuilding the distribution.
    foreach($folder in @('mods','profiles')) {
        if (-not (Test-Path -LiteralPath (Join-Path $dest $folder))) {
            Copy-Item -LiteralPath (Join-Path 'package' $folder) -Destination $dest -Recurse
        }
    }
    Copy-Item -LiteralPath 'target\release\dinput8.dll' -Destination (Join-Path $dest 'dlmod_startup.dll') -Force
    & (Join-Path $PSScriptRoot 'third-party.ps1') -Destination (Join-Path $dest 'licenses')
    # Build a clean downloadable package, never including local settings or installed mods.
    $stage=Join-Path $PSScriptRoot ('dist\package-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $stage | Out-Null
    try {
        foreach($name in @('deathloop-mod-manager.exe','dlmod_runtime.dll','dlmod_startup.dll','README.md','licenses')) {
            Copy-Item -LiteralPath (Join-Path $dest $name) -Destination $stage -Recurse
        }
        foreach($name in @('mods','profiles')) {
            Copy-Item -LiteralPath (Join-Path $PSScriptRoot "package\$name") -Destination $stage -Recurse
        }
        Get-ChildItem -LiteralPath $stage -Recurse -File | Where-Object LastWriteTime -LT ([datetime]'1980-01-01') | ForEach-Object { $_.LastWriteTime=[datetime]'1980-01-01' }
        Compress-Archive -Path (Join-Path $stage '*') -DestinationPath (Join-Path $PSScriptRoot 'dist\DeathloopModManager-0.4.1.zip') -Force
    } finally {
        $resolvedStage=(Resolve-Path -LiteralPath $stage).Path
        if (-not $resolvedStage.StartsWith((Join-Path $PSScriptRoot 'dist\package-'),[StringComparison]::OrdinalIgnoreCase)) { throw 'Unexpected staging path' }
        Remove-Item -LiteralPath $resolvedStage -Recurse -Force
    }
    Write-Host "Built: $dest"
} finally { Pop-Location }

param([string]$Destination=(Join-Path $PSScriptRoot 'dist\DeathloopModManager\licenses'))
$ErrorActionPreference='Stop'
$cargo=Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
$json=& $cargo metadata --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml') --format-version 1 --locked --offline --filter-platform x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw 'Dependency metadata failed' }
$metadata=$json | ConvertFrom-Json
New-Item -ItemType Directory -Path $Destination -Force | Out-Null
foreach($package in $metadata.packages) {
    if (-not $package.source) { continue }
    $source=Split-Path -Parent $package.manifest_path
    $licenses=@(Get-ChildItem -LiteralPath $source | Where-Object Name -Match '^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE)')
    if ($licenses.Count -eq 0 -and $package.name -eq 'mlua-sys') {
        # mlua-sys is in the mlua repository and declares its repository-wide MIT license.
        $parent=$metadata.packages | Where-Object name -eq 'mlua' | Select-Object -First 1
        $licenses=@(Get-ChildItem -LiteralPath (Split-Path -Parent $parent.manifest_path) -File | Where-Object Name -Match '^LICENSE')
    }
    if ($licenses.Count -eq 0) {
        $extra=Join-Path $PSScriptRoot ('third-party-notices\'+$package.name)
        if (Test-Path -LiteralPath $extra) { $licenses=@(Get-ChildItem -LiteralPath $extra -File) }
    }
    if ($licenses.Count -eq 0) { throw "License notice not found for $($package.name)" }
    $folder=Join-Path $Destination "$($package.name)-$($package.version)"
    New-Item -ItemType Directory -Path $folder -Force | Out-Null
    foreach($file in $licenses) { Copy-Item -LiteralPath $file.FullName -Destination $folder -Force -Recurse }
    Copy-Item -LiteralPath $package.manifest_path -Destination (Join-Path $folder 'package-metadata.toml') -Force
    if ($package.name -eq 'lua-src') {
        $luaHeader=Join-Path $source 'lua-5.4.8\lua.h'
        if (Test-Path -LiteralPath $luaHeader) { Copy-Item -LiteralPath $luaHeader -Destination (Join-Path $folder 'Lua-5.4-copyright-header.h') -Force }
    }
}

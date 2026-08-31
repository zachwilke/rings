# Detect arch and install rings from the latest GitHub Release.
# Mapping (PROCESSOR_ARCHITECTURE → asset):
#   AMD64|x86_64   rings-x86_64-pc-windows-msvc.exe.zip
#   ARM64          (no asset — fail; do not install the x64 exe)
# Works with:  irm https://raw.githubusercontent.com/zachwilke/rings/main/install.ps1 | iex
$ErrorActionPreference = "Stop"

function Get-RingsAssetName {
    param(
        [string]$Os,
        [string]$Arch
    )
    if ($Os -notin @("Windows", "windows")) {
        throw "rings-install: unsupported OS: $Os (this script is for Windows; use install.sh on Linux/macOS)"
    }
    switch -Regex ($Arch) {
        "^(AMD64|amd64|x86_64)$" { return "rings-x86_64-pc-windows-msvc.exe.zip" }
        "^(ARM64|arm64)$" {
            throw "rings-install: no Windows ARM64 build yet (arch=$Arch); refuse to install the x64 exe"
        }
        default { throw "rings-install: unsupported Windows arch: $Arch" }
    }
}

function Test-UserWritableDirectory {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        return $false
    }
    $probe = Join-Path $Path ".rings-write-test"
    try {
        [System.IO.File]::WriteAllText($probe, "")
        Remove-Item -LiteralPath $probe -Force
        return $true
    } catch {
        return $false
    }
}

if ($args.Count -ge 1 -and ($args[0] -eq "-PrintAsset" -or $args[0] -eq "--print-asset")) {
    $os = if ($args.Count -ge 2) { [string]$args[1] } else { "Windows" }
    $arch = if ($args.Count -ge 3) { [string]$args[2] } else { $env:PROCESSOR_ARCHITECTURE }
    Write-Output (Get-RingsAssetName -Os $os -Arch $arch)
    return
}

$os = "Windows"
$arch = $env:PROCESSOR_ARCHITECTURE
if (-not $arch) {
    throw "rings-install: PROCESSOR_ARCHITECTURE is empty"
}

$asset = Get-RingsAssetName -Os $os -Arch $arch
$repo = "zachwilke/rings"

if ($env:RING_VERSION) {
    $tag = $env:RING_VERSION
    if (-not $tag.StartsWith("v")) { $tag = "v$tag" }
    $api = "https://api.github.com/repos/$repo/releases/tags/$tag"
} else {
    $api = "https://api.github.com/repos/$repo/releases/latest"
}

$headers = @{
    "User-Agent" = "rings-install"
    "Accept"     = "application/vnd.github+json"
}
$release = Invoke-RestMethod -Uri $api -Headers $headers
$tag = [string]$release.tag_name
if (-not $tag) {
    throw "rings-install: could not read tag_name from GitHub release JSON"
}
if ($tag -notmatch '^[A-Za-z0-9._-]+$') {
    throw "rings-install: unexpected release tag: $tag"
}

$found = $release.assets | Where-Object { $_.name -eq $asset }
if (-not $found) {
    throw "rings-install: release $tag has no asset $asset"
}

$url = "https://github.com/$repo/releases/download/$tag/$asset"
$zip = Join-Path ([System.IO.Path]::GetTempPath()) "rings-$tag.zip"
Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
if (-not (Test-Path -LiteralPath $zip) -or (Get-Item -LiteralPath $zip).Length -le 0) {
    throw "rings-install: download was empty: $url"
}

$extract = Join-Path ([System.IO.Path]::GetTempPath()) "rings-extract-$tag"
if (Test-Path -LiteralPath $extract) {
    Remove-Item -LiteralPath $extract -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $extract | Out-Null
Expand-Archive -Force -Path $zip -DestinationPath $extract

$exe = Get-ChildItem -LiteralPath $extract -Filter "rings.exe" -Recurse | Select-Object -First 1
if (-not $exe) {
    throw "rings-install: zip did not contain rings.exe"
}

if ($env:RING_PREFIX) {
    $dir = $env:RING_PREFIX
} else {
    $dir = Join-Path $env:LOCALAPPDATA "rings"
}
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$dest = Join-Path $dir "rings.exe"
Copy-Item -Force -LiteralPath $exe.FullName -Destination $dest

Write-Host "installed $dest ($tag)"
& $dest --version

$pathDirs = @($env:Path -split ";" | Where-Object { $_ })
$onPath = $false
foreach ($p in $pathDirs) {
    $trim = $p.TrimEnd("\")
    if ($trim -and ($trim -ieq $dir.TrimEnd("\"))) {
        $onPath = $true
        break
    }
}

if (-not $onPath) {
    $copied = $false
    foreach ($p in $pathDirs) {
        if ($p -and (Test-UserWritableDirectory $p)) {
            Copy-Item -Force -LiteralPath $dest -Destination (Join-Path $p "rings.exe")
            Write-Host "also copied to $p (already on PATH)"
            $copied = $true
            break
        }
    }
    if (-not $copied) {
        Write-Host "note: $dir is not on PATH. Add it to your user PATH, or copy rings.exe into a PATH directory."
        Write-Host "  [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path','User') + ';$dir', 'User')"
    }
}

Write-Host "next: $dest C:\"
Write-Host "  or: .\rings.exe C:\"

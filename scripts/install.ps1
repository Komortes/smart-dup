param(
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [string]$InstallDir = "$env:USERPROFILE\bin",
    [string]$Repo = "oleksandrskoruk/smart-dup"
)

$ErrorActionPreference = "Stop"

if ($Version.StartsWith("v")) {
    $Tag = $Version
} else {
    $Tag = "v$Version"
}

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
switch ($arch) {
    "X64" { $target = "x86_64-pc-windows-msvc" }
    default { throw "Unsupported Windows architecture: $arch" }
}

$archive = "smart-dup-$Tag-$target.zip"
$url = "https://github.com/$Repo/releases/download/$Tag/$archive"

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("smartdup-install-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmpDir | Out-Null

try {
    $zipPath = Join-Path $tmpDir $archive
    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $zipPath

    Expand-Archive -Path $zipPath -DestinationPath $tmpDir -Force

    $exe = Get-ChildItem -Path $tmpDir -Recurse -Filter "smart-dup.exe" | Select-Object -First 1
    if (-not $exe) {
        throw "smart-dup.exe not found in archive"
    }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $destination = Join-Path $InstallDir "smart-dup.exe"
    Copy-Item -Path $exe.FullName -Destination $destination -Force

    Write-Host "Installed to $destination"
    Write-Host "If needed, add $InstallDir to PATH and restart terminal."
} finally {
    if (Test-Path $tmpDir) {
        Remove-Item -Path $tmpDir -Recurse -Force
    }
}

param(
  [string]$Version
)

$ErrorActionPreference = "Stop"

$repo = if ($env:SYFT_RELEASE_REPO) { $env:SYFT_RELEASE_REPO } else { "chaqchase/syft" }
$installDir = if ($env:SYFT_INSTALL_DIR) { $env:SYFT_INSTALL_DIR } else { Join-Path $HOME ".local\bin" }

function Get-LatestTag {
  $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest"
  return $release.tag_name
}

function Get-Target {
  $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
  switch ($arch) {
    "X64" { return "x86_64-pc-windows-msvc" }
    default { throw "Unsupported Windows architecture: $arch" }
  }
}

if (-not $Version) {
  $Version = Get-LatestTag
}

if ($Version.StartsWith("v")) {
  $tag = $Version
  $versionNumber = $Version.Substring(1)
} else {
  $tag = "v$Version"
  $versionNumber = $Version
}

$target = Get-Target
$asset = "syft-$versionNumber-$target.zip"
$url = "https://github.com/$repo/releases/download/$tag/$asset"

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("syft-install-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null

try {
  $archivePath = Join-Path $tempRoot $asset
  $extractPath = Join-Path $tempRoot "extract"

  Write-Host "Downloading $url"
  Invoke-WebRequest -Uri $url -OutFile $archivePath
  Expand-Archive -Path $archivePath -DestinationPath $extractPath -Force

  $binaryPath = Join-Path $extractPath "syft-$versionNumber-$target\syft.exe"
  New-Item -ItemType Directory -Force -Path $installDir | Out-Null
  Copy-Item $binaryPath (Join-Path $installDir "syft.exe") -Force

  Write-Host "Installed syft to $installDir\syft.exe"
  if (-not ($env:PATH -split ';' | Where-Object { $_ -eq $installDir })) {
    Write-Host "Add $installDir to PATH if it is not there already"
  }
}
finally {
  Remove-Item -Recurse -Force $tempRoot
}

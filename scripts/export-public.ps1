<#
.SYNOPSIS
  Erzeugt den öffentlichen Stand von OSULZER ImageOptimizer mit frischer Git-Historie.
.DESCRIPTION
  Exportiert HEAD ohne interne Planungsdokumente (docs/superpowers, .superpowers) in einen leeren Ordner,
  bricht ab, wenn LAN-Adressen, Benutzerpfade oder E-Mail-Adressen darin vorkommen, und legt dort ein
  Git-Repository mit genau einem Commit an. Gepusht wird nicht.
.EXAMPLE
  powershell -File scripts/export-public.ps1 -Target ..\osulzer-imageoptimizer-public
#>
param(
    [Parameter(Mandatory = $true)][string]$Target
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

if (Test-Path $Target) {
    if (@(Get-ChildItem -LiteralPath $Target -Force).Count -gt 0) { throw "Target folder is not empty: $Target" }
} else {
    New-Item -ItemType Directory -Path $Target | Out-Null
}
$Target = (Resolve-Path $Target).Path

$version = (Select-String -Path (Join-Path $repo 'Cargo.toml') -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value

$tar = Join-Path $env:TEMP "osulzer-export-$PID.tar"
git -C $repo archive --format=tar -o $tar HEAD
if ($LASTEXITCODE -ne 0) { throw 'git archive failed' }
tar -xf $tar -C $Target
Remove-Item $tar
foreach ($internal in @('docs/superpowers', '.superpowers')) {
    $path = Join-Path $Target $internal
    if (Test-Path $path) { Remove-Item -Recurse -Force $path }
}

# Nichts Privates veröffentlichen.
# Die Muster entstehen erst hier aus der eigenen Umgebung, damit das Skript selbst nichts verrät:
# Benutzername, Netz der internen Git-Adresse (z. B. 192.168.x.) und die Git-E-Mail samt Domain.
$patterns = @()
# Benutzername in Pfaden, auch maskiert (C:\\Users\\name) und in URLs (/name/).
if (-not $env:USERNAME) { throw 'USERNAME is not set; refusing to export unchecked.' }
$user = [regex]::Escape($env:USERNAME)
$patterns += "Users[\\/]+$user\b"
$patterns += "/$user/"
# Interne Git-Adresse: der Rechnername bzw. bei einer IP ihr Netz (z. B. 192.168.x.).
$origin = git -C $repo remote get-url origin 2>$null
if ($origin -match '://(?:[^@/]+@)?(\d+\.\d+\.\d+)\.\d+') {
    $patterns += ([regex]::Escape($Matches[1]) + '\.')
} elseif ($origin -match '://(?:[^@/]+@)?([^:/]+)') {
    $patterns += [regex]::Escape($Matches[1])
} else {
    throw 'No origin remote; cannot check for the internal server address.'
}
# E-Mail-Adressen: jede außer den erlaubten (Attribution, Beispieladressen).
# Die Endung muss aus Buchstaben bestehen, sonst gilt z. B. „wght@12..96“ in einer Google-Fonts-Adresse als Treffer.
$patterns += '[A-Za-z0-9._%+-]+@(?!anthropic\.com\b|example\.(?:com|org)\b)[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}\b'
$mail = git -C $repo config user.email 2>$null
if ($mail -match '^(.+)@(.+)$') { $patterns += ('@' + [regex]::Escape($Matches[2])) }
# UTF-16-Dateien würde Select-String falsch lesen; es gibt im Projekt keine, also auffallen lassen.
$utf16 = Get-ChildItem -LiteralPath $Target -Recurse -File | Where-Object {
    $b = [System.IO.File]::ReadAllBytes($_.FullName) | Select-Object -First 2
    $b.Count -eq 2 -and (($b[0] -eq 0xFF -and $b[1] -eq 0xFE) -or ($b[0] -eq 0xFE -and $b[1] -eq 0xFF))
}
if ($utf16) { $utf16 | ForEach-Object { Write-Host "UTF-16 file: $($_.FullName)" }; throw 'UTF-16 files cannot be checked.' }
$hits = Get-ChildItem -LiteralPath $Target -Recurse -File | Select-String -Pattern $patterns
if ($hits) {
    $hits | ForEach-Object { Write-Host ("{0}:{1}: {2}" -f $_.Path, $_.LineNumber, $_.Line.Trim()) }
    throw 'Private data found, nothing committed. Fix the files above and run again.'
}

git -C $Target init -q -b main
git -C $Target add -A
$message = @"
OSULZER ImageOptimizer $version

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
"@
git -C $Target commit -q -m $message
if ($LASTEXITCODE -ne 0) { throw 'git commit failed' }
Write-Host "Public tree with one commit in $Target"
Write-Host "Next: git -C `"$Target`" remote add origin <url>; git -C `"$Target`" push -u origin main"

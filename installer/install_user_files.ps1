# Run by the installer as the original unelevated user (2026-10-19, security review #4).
# Copies the conversion server, dictionaries and model from the protected payload in Program Files into %APPDATA%\HaniWord.
# The elevated installer only writes under Program Files; anything in the user's profile is written here, with the user's
# own rights, so links planted in the profile cannot redirect an administrator's writes or deletes.
# -Symbols: the symbol preset chosen in the installer (full / half / marks); empty when the user took the recommended setup
# -Languages: comma-separated accent languages chosen in the installer (fr,de,...); empty for English only
# -Domains: comma-separated domains the user often types, chosen in the installer (law,math,...); empty when none was chosen
param([Parameter(Mandatory=$true)][string]$Payload, [ValidateSet("", "full", "half", "marks")][string]$Symbols = "",
      [ValidatePattern('^([a-z]+(,[a-z]+)*)?$')][string]$Languages = "",
      [ValidatePattern('^([a-z]+(,[a-z]+)*)?$')][string]$Domains = "")
$ErrorActionPreference = "Stop"

if (-not ("HaniWordInstallToken" -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Security.Principal;
public static class HaniWordInstallToken {
    [DllImport("advapi32.dll", SetLastError=true)]
    static extern bool GetTokenInformation(IntPtr token, int kind, out int value, int size, out int written);
    public static void RequireUnelevated() {
        using (var identity = WindowsIdentity.GetCurrent()) {
            int value, written;
            if (!GetTokenInformation(identity.Token, 20, out value, 4, out written) || value != 0)
                throw new InvalidOperationException("User files must be installed as the original unelevated user");
        }
    }
}
'@
}
[HaniWordInstallToken]::RequireUnelevated()

function Test-Linked([IO.FileSystemInfo]$Item) { [bool]($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) }

# The payload must be the protected copy under Program Files\HaniWord\Releases\<version>\user-files
$protectedRoot = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)) "HaniWord"
$Payload = [IO.Path]::GetFullPath($Payload).TrimEnd('\')
if ([IO.Path]::GetFileName($Payload) -ne "user-files" -or
    [IO.Path]::GetDirectoryName([IO.Path]::GetDirectoryName($Payload)) -ne "$protectedRoot\Releases") { throw "Unexpected payload location" }
for ($item = Get-Item -LiteralPath $Payload -Force; $null -ne $item; $item = $item.Parent) {
    if (Test-Linked $item) { throw "Linked payload path rejected" }
}

# Target: this user's %APPDATA%\HaniWord. No link anywhere on the way, and none inside it
$appData = [IO.Path]::GetFullPath($env:APPDATA).TrimEnd('\')
$target = Join-Path $appData "HaniWord"
for ($item = Get-Item -LiteralPath $appData -Force; $null -ne $item; $item = $item.Parent) {
    if (Test-Linked $item) { throw "Linked profile path rejected" }
}
if (Test-Path -LiteralPath $target) {
    if (Test-Linked (Get-Item -LiteralPath $target -Force)) { throw "Linked data folder rejected" }
} else {
    $null = New-Item -ItemType Directory -Path $target
}

function Assert-NoLinkBelow([string]$Path) {
    # every existing folder between the data folder and Path (inclusive) must be a real folder
    $relative = $Path.Substring($target.Length).TrimStart('\')
    $current = $target
    foreach ($part in ($relative -split '\\' | Where-Object { $_ })) {
        $current = Join-Path $current $part
        if (Test-Path -LiteralPath $current) {
            if (Test-Linked (Get-Item -LiteralPath $current -Force)) { throw "Linked entry rejected: $current" }
        }
    }
}

# Files that older versions placed in %APPDATA%\HaniWord and that now live only in Program Files
foreach ($name in @("launcher.exe","ui.exe","launch.vbs","setup_tasks.ps1","remove_tasks.ps1","azookey.dll","azookey32.dll","azookey-settings.exe","tex_commands.txt")) {
    $old = Join-Path $target $name
    if (Test-Path -LiteralPath $old) {
        Assert-NoLinkBelow $old
        Remove-Item -LiteralPath $old -Force
    }
}

# The domain dictionaries (domain\) hold only files from the payload: drop the old set first, so that a dictionary or table
# removed in this version does not stay behind and get read. Nothing of the user's lives there.
$domain = Join-Path $target "domain"
if (Test-Path -LiteralPath $domain) {
    Assert-NoLinkBelow $domain
    foreach ($item in Get-ChildItem -LiteralPath $domain -Recurse -Force) {
        if (Test-Linked $item) { throw "Linked entry rejected: $($item.FullName)" }
    }
    Remove-Item -LiteralPath $domain -Recurse -Force
}

# Copy the payload (program files only; settings and learning data are not in it and are never overwritten)
$copied = 0
foreach ($file in Get-ChildItem -LiteralPath $Payload -Recurse -File -Force) {
    if (Test-Linked $file) { throw "Linked payload file rejected" }
    $relative = $file.FullName.Substring($Payload.Length).TrimStart('\')
    $destination = Join-Path $target $relative
    $directory = [IO.Path]::GetDirectoryName($destination)
    Assert-NoLinkBelow $directory
    if (-not (Test-Path -LiteralPath $directory)) { $null = New-Item -ItemType Directory -Path $directory }
    Assert-NoLinkBelow $destination
    Copy-Item -LiteralPath $file.FullName -Destination $destination -Force
    $copied++
}
if ($copied -eq 0) { throw "Empty payload" }

# A new install or update tries the GPU (and the AVX2 CPU build) again: drop the markers the launcher leaves after a server crashed while loading
foreach ($name in @("backend_failed.txt", "cpu_avx2_failed.txt")) {
    $marker = Join-Path $target $name
    if (Test-Path -LiteralPath $marker) {
        Assert-NoLinkBelow $marker
        Remove-Item -LiteralPath $marker -Force
    }
}

# Settings chosen in the installer: the launcher folds install_choices.json into settings.json at its next start and deletes it
$chosen = @{}
if ($Symbols) {
    $all = '!"#$%&''()*+:;<=>?@\^_`{|}~'
    $chosen.fullwidth_symbols = switch ($Symbols) { "full" { $all } "half" { "" } "marks" { "!?~" } }
}
if ($Languages) {
    [string[]]$codes = $Languages.Split(',')
    foreach ($code in $codes) {
        if ($code -notin @("fr","de","es","it","pt","nordic","pl","cs","tr","pinyin")) { throw "Unknown language: $code" }
    }
    $chosen.latin_languages = $codes
}
# Only the domains this payload ships (domain\domains.txt, the list the conversion server reads; the installer page shows the same ones).
# The key is left out when none was chosen
if ($Domains) {
    $listFile = Join-Path $Payload "domain\domains.txt"
    $published = @()
    if (Test-Path -LiteralPath $listFile) {
        $published = @(Get-Content -LiteralPath $listFile -Encoding utf8 | ForEach-Object { $_.Trim() } | Where-Object { $_ -and -not $_.Contains("#") })
    }
    [string[]]$names = $Domains.Split(',')
    foreach ($name in $names) {
        if ($name -notin $published) { throw "Unknown domain: $name" }
    }
    $chosen.domains_focus = $names
}
if ($chosen.Count -gt 0) {
    $choices = Join-Path $target "install_choices.json"
    Assert-NoLinkBelow $choices
    $json = $chosen | ConvertTo-Json -Compress
    [IO.File]::WriteAllText($choices, $json, (New-Object Text.UTF8Encoding($false)))
}
Write-Output "Installed $copied files into $target"

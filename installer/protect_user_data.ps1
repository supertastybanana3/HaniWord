param([string]$App, [string]$Launcher)
$ErrorActionPreference = "Stop"

function Protect-HaniWordPrivateData([Parameter(Mandatory=$true)][string]$Directory, [string]$ExportLauncher) {
    if (-not ("HaniWordPrivacyToken" -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Security.Principal;
public static class HaniWordPrivacyToken {
    [DllImport("advapi32.dll", SetLastError=true)]
    static extern bool GetTokenInformation(IntPtr token, int kind, out int value, int size, out int written);
    public static void RequireUnelevated() {
        using (var identity = WindowsIdentity.GetCurrent()) {
            int value, written;
            if (!GetTokenInformation(identity.Token, 20, out value, 4, out written) || value != 0)
                throw new InvalidOperationException("Privacy migration must run as the original unelevated user");
        }
    }
}
'@
    }
    # Never apply user-directory ACL changes with an administrator token.
    [HaniWordPrivacyToken]::RequireUnelevated()
    $path = [IO.Path]::GetFullPath($Directory).TrimEnd('\')
    if ([IO.Path]::GetFileName($path) -ne "HaniWord") { throw "Unexpected privacy directory" }
    if ($path -ne [IO.Path]::GetFullPath((Join-Path $env:APPDATA "HaniWord"))) { throw "Privacy target does not belong to the original user" }
    $root = Get-Item -LiteralPath $path -Force
    if (-not $root.PSIsContainer) { throw "Privacy root must be a directory" }
    for ($parent = $root; $null -ne $parent; $parent = $parent.Parent) {
        if ($parent.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Linked privacy path rejected" }
    }
    $entries = New-Object 'Collections.Generic.List[IO.FileSystemInfo]'
    $pending = New-Object 'Collections.Generic.Stack[IO.DirectoryInfo]'
    $entries.Add($root)
    $pending.Push($root)
    while ($pending.Count) {
        foreach ($entry in $pending.Pop().GetFileSystemInfos()) {
            if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Linked privacy entry rejected" }
            $entries.Add($entry)
            if ($entries.Count -gt 100000) { throw "Privacy tree limit exceeded" }
            if ($entry -is [IO.DirectoryInfo]) { $pending.Push($entry) }
        }
    }
    $packageSids = @("S-1-15-2-1", "S-1-15-2-2", "S-1-5-12")
    foreach ($entry in $entries) {
        $acl = $entry.GetAccessControl([Security.AccessControl.AccessControlSections]::Access)
        $inherited = @($acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]) |
            Where-Object { $_.IsInherited -and $_.IdentityReference.Value -in $packageSids -and $_.AccessControlType -eq "Allow" })
        if ($entry.FullName -eq $path -or $inherited.Count) {
            $acl.SetAccessRuleProtection($true,$true)
        }
        foreach ($rule in @($acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]))) {
            if ($rule.IdentityReference.Value -in $packageSids -and $rule.AccessControlType -eq "Allow") {
                $acl.RemoveAccessRuleSpecific($rule)
            }
        }
        $entry.SetAccessControl($acl)
    }
    # This dedicated folder contains only the allowlisted input-preference snapshot.
    $public = Join-Path $path "client"
    $null = New-Item -ItemType Directory -Path $public -Force
    if ($ExportLauncher) {
        $protectedRoot = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)) "HaniWord"
        $exe = Get-Item -LiteralPath $ExportLauncher
        if ($exe.Name -ne "launcher.exe" -or -not $exe.FullName.StartsWith($protectedRoot + '\',[StringComparison]::OrdinalIgnoreCase)) { throw "Untrusted settings exporter" }
        for ($parent = $exe; $null -ne $parent; $parent = $(if ($parent -is [IO.FileInfo]) { $parent.Directory } else { $parent.Parent })) {
            if ($parent.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Linked settings exporter rejected" }
        }
        & $exe.FullName --export-client-settings
        if ($LASTEXITCODE -ne 0) { throw "Client settings export failed" }
    }
    $publicDirectory = Get-Item -LiteralPath $public
    $acl = $publicDirectory.GetAccessControl([Security.AccessControl.AccessControlSections]::Access)
    foreach ($sid in $packageSids) {
        $rule = New-Object Security.AccessControl.FileSystemAccessRule(
            (New-Object Security.Principal.SecurityIdentifier($sid)), "ReadAndExecute", "ContainerInherit,ObjectInherit", "None", "Allow")
        $acl.AddAccessRule($rule)
    }
    $publicDirectory.SetAccessControl($acl)
    foreach ($entry in Get-ChildItem -LiteralPath $public -Force -Recurse) {
        if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Linked public settings rejected" }
        $acl = $entry.GetAccessControl([Security.AccessControl.AccessControlSections]::Access)
        $acl.SetAccessRuleProtection($false,$false)
        $entry.SetAccessControl($acl)
    }
}

if ($App) {
    if (-not $Launcher) { throw "Protected settings exporter is required" }
    Protect-HaniWordPrivateData -Directory $App -ExportLauncher $Launcher
}

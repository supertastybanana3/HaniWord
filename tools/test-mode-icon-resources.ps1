param([Parameter(Mandatory=$true)][string[]]$Dll)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Runtime.InteropServices;
public static class ModeIconResourceTest {
    [DllImport("kernel32", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr LoadLibraryEx(string file, IntPtr reserved, uint flags);
    [DllImport("kernel32")] static extern bool FreeLibrary(IntPtr module);
    [DllImport("user32", EntryPoint="LoadImageW", SetLastError=true)]
    static extern IntPtr ResourceIcon(IntPtr module, IntPtr id, uint type, int x, int y, uint flags);
    [DllImport("user32", EntryPoint="LoadImageW", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr FileIcon(IntPtr module, string file, uint type, int x, int y, uint flags);
    [DllImport("user32")] static extern bool DestroyIcon(IntPtr icon);
    public static void Verify(string dll, string resources) {
        // DATAFILE never executes the DLL, including when checking the other architecture.
        IntPtr module = LoadLibraryEx(dll, IntPtr.Zero, 2);
        if (module == IntPtr.Zero) throw new Exception("LoadLibraryEx: " + Marshal.GetLastWin32Error());
        try {
            string[] names = {"kana_black", "latin_black", "kana_white", "latin_white"};
            int[] sizes = {0,16,20,24,32,40,48,64,96,128,256};
            for (int i=0; i<names.Length; i++) foreach (int size in sizes) {
                IntPtr actual = ResourceIcon(module, new IntPtr(102+i), 1, size, size, 0);
                IntPtr expected = FileIcon(IntPtr.Zero, System.IO.Path.Combine(resources,names[i]+".ico"),1,size,size,0x10);
                try {
                    if (actual == IntPtr.Zero || expected == IntPtr.Zero) throw new Exception("LoadImage failed");
                    using (Icon a = Icon.FromHandle(actual)) using (Icon e = Icon.FromHandle(expected))
                    using (Bitmap ab = a.ToBitmap()) using (Bitmap eb = e.ToBitmap()) {
                        int expectedSize = size == 0 ? 32 : size;
                        if (ab.Width != expectedSize || ab.Height != expectedSize) throw new Exception("Wrong icon size");
                        int visible = 0;
                        for (int y=0; y<expectedSize; y++) for (int x=0; x<expectedSize; x++) {
                            Color pixel = ab.GetPixel(x,y);
                            if (pixel.A > 0) visible++;
                            if (pixel.ToArgb() != eb.GetPixel(x,y).ToArgb())
                                throw new Exception("Stale or mismatched resource: " + names[i] + " " + size);
                        }
                        if (visible == 0) throw new Exception("Blank native icon");
                    }
                } finally {
                    if (actual != IntPtr.Zero) DestroyIcon(actual);
                    if (expected != IntPtr.Zero) DestroyIcon(expected);
                }
            }
        } finally { FreeLibrary(module); }
    }
}
'@
$resources = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\crates\client\res'))
foreach ($file in $Dll) {
    [ModeIconResourceTest]::Verify([IO.Path]::GetFullPath($file), $resources)
    Write-Output "Verified 44 native icon loads (including default size): $file"
}

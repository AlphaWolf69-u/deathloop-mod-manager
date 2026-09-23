param([Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class IconResource {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern IntPtr BeginUpdateResource(string file, bool deleteExistingResources);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool UpdateResource(IntPtr update, IntPtr type, IntPtr name, ushort language, byte[] data, uint length);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool EndUpdateResource(IntPtr update, bool discard);
}
'@

# Scale the supplied artwork for the Windows executable icon.
$source=Join-Path $PSScriptRoot 'launcher\assets\manager-icon.png'
$original=[System.Drawing.Bitmap]::FromFile($source)
$bmp=New-Object System.Drawing.Bitmap 64,64
$graphics=[System.Drawing.Graphics]::FromImage($bmp)
$graphics.InterpolationMode=[System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$graphics.DrawImage($original,0,0,64,64)
$graphics.Dispose()
$original.Dispose()
$stream=New-Object System.IO.MemoryStream
$bmp.Save($stream,[System.Drawing.Imaging.ImageFormat]::Png)
$image=$stream.ToArray()
$stream.Dispose(); $bmp.Dispose()
$group=New-Object byte[] 20
$group[2]=1
[Array]::Copy([BitConverter]::GetBytes([uint16]1),0,$group,4,2)
$group[6]=64; $group[7]=64
[Array]::Copy([BitConverter]::GetBytes([uint16]1),0,$group,10,2)
[Array]::Copy([BitConverter]::GetBytes([uint16]32),0,$group,12,2)
[Array]::Copy([BitConverter]::GetBytes([uint32]$image.Length),0,$group,14,4)
[Array]::Copy([BitConverter]::GetBytes([uint16]1),0,$group,18,2)
$update=[IconResource]::BeginUpdateResource($Executable,$false)
if ($update -eq [IntPtr]::Zero) { throw "Cannot open icon resources: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
$success=$false
try {
    if (-not [IconResource]::UpdateResource($update,[IntPtr]3,[IntPtr]1,0,$image,[uint32]$image.Length)) { throw "Cannot write icon image: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    if (-not [IconResource]::UpdateResource($update,[IntPtr]14,[IntPtr]1,0,$group,[uint32]$group.Length)) { throw "Cannot write icon group: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    $success=$true
} finally {
    if (-not [IconResource]::EndUpdateResource($update,-not $success)) { throw "Cannot finish icon resource: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
}

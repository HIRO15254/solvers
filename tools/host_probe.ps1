param(
    [string]$Volume = 'C:',
    [switch]$IncludeGpu
)

$ErrorActionPreference = 'Stop'

$nativeSource = @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class SolversHostProbe {
    [StructLayout(LayoutKind.Sequential)]
    public struct MemoryStatus {
        public uint Length, Load;
        public ulong TotalPhysical, AvailablePhysical, TotalPageFile, AvailablePageFile;
        public ulong TotalVirtual, AvailableVirtual, Extended;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct PowerStatus {
        public byte ACLineStatus, BatteryFlag, BatteryLifePercent, SystemStatusFlag;
        public uint BatteryLifeTime, BatteryFullLifeTime;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GlobalMemoryStatusEx(ref MemoryStatus status);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetSystemPowerStatus(out PowerStatus status);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetLogicalProcessorInformation(IntPtr buffer, ref uint length);

    public static MemoryStatus Memory() {
        var status = new MemoryStatus();
        status.Length = (uint)Marshal.SizeOf(status);
        if (!GlobalMemoryStatusEx(ref status))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        return status;
    }

    public static PowerStatus Power() {
        PowerStatus status;
        if (!GetSystemPowerStatus(out status))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        return status;
    }

    public static int PhysicalCores() {
        uint length = 0;
        GetLogicalProcessorInformation(IntPtr.Zero, ref length);
        if (length == 0)
            throw new Win32Exception(Marshal.GetLastWin32Error());
        var buffer = Marshal.AllocHGlobal((int)length);
        try {
            if (!GetLogicalProcessorInformation(buffer, ref length))
                throw new Win32Exception(Marshal.GetLastWin32Error());
            int count = 0;
            int size = IntPtr.Size == 8 ? 32 : 24;
            for (int offset = 0; offset < length; offset += size)
                if (Marshal.ReadInt32(buffer, offset + IntPtr.Size) == 0)
                    count++;
            return count;
        } finally {
            Marshal.FreeHGlobal(buffer);
        }
    }
}
'@

Add-Type -TypeDefinition $nativeSource
$memory = [SolversHostProbe]::Memory()
$power = [SolversHostProbe]::Power()
$drive = [IO.DriveInfo]::new($Volume)
if (-not $drive.IsReady -or $drive.DriveType -ne [IO.DriveType]::Fixed) {
    throw "Volume $Volume is not a ready fixed drive"
}

$cpuModel = Get-ItemPropertyValue 'HKLM:\HARDWARE\DESCRIPTION\System\CentralProcessor\0' -Name ProcessorNameString
$powerSchemeText = (powercfg /getactivescheme) -join ' '
$powerSchemeGuid = if ($powerSchemeText -match '[0-9a-fA-F]{8}-(?:[0-9a-fA-F]{4}-){3}[0-9a-fA-F]{12}') { $Matches[0] } else { $null }

$gpu = @{ status = 'not_applicable'; reason = 'CPU-only measurement' }
$missing = @()
if ($IncludeGpu) {
    $nvidiaSmi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
    if ($nvidiaSmi) {
        $gpuText = & $nvidiaSmi.Source --query-gpu=name,memory.total,memory.free,driver_version --format=csv,noheader,nounits 2>&1
        if ($LASTEXITCODE -eq 0) {
            $gpu = @{ status = 'observed'; vendor_tool = 'nvidia-smi'; raw_csv_mib = @($gpuText) }
        } else {
            $gpu = @{ status = 'unavailable'; reason = "nvidia-smi exit code $LASTEXITCODE" }
        }
    } else {
        $gpu = @{ status = 'unavailable'; reason = 'nvidia-smi is not installed or this is not an NVIDIA host' }
    }
    if ($gpu.status -eq 'unavailable') {
        $missing += @{ field = 'GPU/VRAM'; reason = $gpu.reason; alternative = 'Use the active GPU vendor tool before a GPU measurement' }
    }
}

[pscustomobject]@{
    observed_at = (Get-Date).ToString('o')
    os = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    architecture = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
    cpu = @{ model = $cpuModel; physical_cores = [SolversHostProbe]::PhysicalCores(); logical_cores = [Environment]::ProcessorCount }
    memory = @{
        total_physical_bytes = $memory.TotalPhysical
        available_physical_bytes = $memory.AvailablePhysical
        memory_load_percent = $memory.Load
        total_page_file_bytes = $memory.TotalPageFile
        available_page_file_bytes = $memory.AvailablePageFile
    }
    disk = @{ volume = $drive.Name; total_bytes = $drive.TotalSize; available_bytes = $drive.AvailableFreeSpace }
    power = @{ ac_line_status = $power.ACLineStatus; battery_flag = $power.BatteryFlag; scheme_guid = $powerSchemeGuid; scheme_raw = $powerSchemeText }
    gpu = $gpu
    missing = $missing
} | ConvertTo-Json -Depth 6

param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$ArgumentsJson,
    [Parameter(Mandatory)][string]$Directory,
    [ValidateRange(1,3600)][int]$Wall = 900,
    [ValidateRange(1,3600)][int]$Idle = 180,
    [switch]$MinimalPath
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
if (-not $IsWindows) {
    $ownerCode = @'
import json, os, selectors, signal, subprocess, sys, time
exe, cwd, wall, idle, raw = sys.argv[1:]
wall, idle = int(wall), int(idle)
p = subprocess.Popen([exe, *json.loads(raw)], cwd=cwd, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
selector = selectors.DefaultSelector()
selector.register(p.stdout, selectors.EVENT_READ, sys.stdout.buffer)
selector.register(p.stderr, selectors.EVENT_READ, sys.stderr.buffer)
started = progress = time.monotonic()
total = 0
try:
    while selector.get_map():
        now = time.monotonic()
        if now-started >= wall or now-progress >= idle:
            raise RuntimeError('Owned command wall/idle timeout')
        for key, event in selector.select(0.05):
            chunk = os.read(key.fileobj.fileno(), 4096)
            if not chunk:
                selector.unregister(key.fileobj)
                continue
            total += len(chunk)
            if total > 16*1024*1024:
                raise RuntimeError('Owned output limit exceeded')
            progress = time.monotonic()
            key.data.write(chunk)
            key.data.flush()
    code = p.wait(timeout=max(0.1, wall-(time.monotonic()-started)))
    try:
        os.killpg(p.pid, 0)
    except ProcessLookupError:
        pass
    else:
        raise RuntimeError('Owned descendant survived root')
    sys.exit(code)
finally:
    try:
        os.killpg(p.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    p.wait(timeout=5)
    selector.close()
    p.stdout.close()
    p.stderr.close()
'@
    & python3 -I -c $ownerCode $Executable $Directory $Wall $Idle $ArgumentsJson
    exit $LASTEXITCODE
}
if ([IntPtr]::Size -ne 8) { throw 'Native owned wrapper requires Windows x64.' }
$arguments = @(ConvertFrom-Json -InputObject $ArgumentsJson)
if ($arguments | Where-Object { $_ -isnot [string] }) { throw 'Arguments must be strings.' }
$root = Split-Path $PSScriptRoot -Parent
$scratch = Join-Path $root ('.ci-work/owner-' + [Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
public sealed class CiOwned : IDisposable {
    [StructLayout(LayoutKind.Sequential)] struct SA { public int size; public IntPtr security; public int inherit; }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct SI {
        public int cb; public string reserved, desktop, title; public int x,y,xs,ys,xc,yc,fill,flags; public short show, reserved2; public IntPtr reserve, input, output, error;
    }
    [StructLayout(LayoutKind.Sequential)] struct SX { public SI startup; public IntPtr attributes; }
    [StructLayout(LayoutKind.Sequential)] struct PI { public IntPtr process, thread; public int pid, tid; }
    [StructLayout(LayoutKind.Sequential)] struct Accounting { public long a,b,c,d; public uint pages,total,active,terminated; }
    [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr CreateJobObjectW(IntPtr a,string b);
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetInformationJobObject(IntPtr h,int cls,IntPtr value,int size);
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr h,int cls,out Accounting value,int size,IntPtr written);
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateJobObject(IntPtr h,uint code);
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool InitializeProcThreadAttributeList(IntPtr list,int count,int flags,ref IntPtr size);
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool UpdateProcThreadAttribute(IntPtr list,uint flags,IntPtr key,IntPtr value,IntPtr size,IntPtr previous,IntPtr returned);
    [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateFileW(string path,uint access,uint share,ref SA sa,uint disposition,uint flags,IntPtr template);
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateProcessW(string app,StringBuilder command,IntPtr pa,IntPtr ta,bool inherit,uint flags,IntPtr env,string cwd,ref SX si,out PI pi);
    [DllImport("kernel32.dll",SetLastError=true)] static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr h,uint ms);
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr h,out uint code);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    IntPtr job, process;
    static void Check(bool success) { if(!success) throw new Win32Exception(Marshal.GetLastWin32Error()); }
    static string Quote(string text) {
        var result = new StringBuilder("\""); int slashes=0;
        foreach(char c in text) { if(c=='\\') { slashes++; continue; } if(c=='\"') result.Append('\\',slashes*2+1); else result.Append('\\',slashes); result.Append(c); slashes=0; }
        return result.Append('\\',slashes*2).Append('"').ToString();
    }
    public CiOwned(string exe,string[] args,string cwd,string stdout,string stderr) {
        IntPtr attributes=IntPtr.Zero, jobPointer=IntPtr.Zero, handlesPointer=IntPtr.Zero, limits=IntPtr.Zero;
        IntPtr input=IntPtr.Zero, output=IntPtr.Zero, error=IntPtr.Zero; PI pi=new PI();
        try {
            job=CreateJobObjectW(IntPtr.Zero,null); Check(job!=IntPtr.Zero);
            limits=Marshal.AllocHGlobal(144); Marshal.Copy(new byte[144],0,limits,144); Marshal.WriteInt32(limits,16,0x2000);
            Check(SetInformationJobObject(job,9,limits,144));
            var sa=new SA {size=Marshal.SizeOf(typeof(SA)),inherit=1};
            input=CreateFileW("NUL",0x80000000,3,ref sa,3,0,IntPtr.Zero);
            output=CreateFileW(stdout,0x40000000,3,ref sa,1,0,IntPtr.Zero);
            error=CreateFileW(stderr,0x40000000,3,ref sa,1,0,IntPtr.Zero);
            Check(input!=new IntPtr(-1)&&output!=new IntPtr(-1)&&error!=new IntPtr(-1));
            IntPtr size=IntPtr.Zero; InitializeProcThreadAttributeList(IntPtr.Zero,2,0,ref size);
            attributes=Marshal.AllocHGlobal(size); Check(InitializeProcThreadAttributeList(attributes,2,0,ref size));
            jobPointer=Marshal.AllocHGlobal(IntPtr.Size); Marshal.WriteIntPtr(jobPointer,job);
            Check(UpdateProcThreadAttribute(attributes,0,new IntPtr(0x2000D),jobPointer,new IntPtr(IntPtr.Size),IntPtr.Zero,IntPtr.Zero));
            handlesPointer=Marshal.AllocHGlobal(IntPtr.Size*3);
            Marshal.WriteIntPtr(handlesPointer,0,input); Marshal.WriteIntPtr(handlesPointer,IntPtr.Size,output); Marshal.WriteIntPtr(handlesPointer,IntPtr.Size*2,error);
            Check(UpdateProcThreadAttribute(attributes,0,new IntPtr(0x20002),handlesPointer,new IntPtr(IntPtr.Size*3),IntPtr.Zero,IntPtr.Zero));
            var si=new SX {startup=new SI {cb=Marshal.SizeOf(typeof(SX)),flags=0x100,input=input,output=output,error=error},attributes=attributes};
            var command=new StringBuilder(Quote(exe)); foreach(string arg in args) command.Append(' ').Append(Quote(arg));
            if(command.Length>=32767) throw new ArgumentException("Windows command exceeds supported bound.");
            Check(CreateProcessW(exe,command,IntPtr.Zero,IntPtr.Zero,true,0x08080004,IntPtr.Zero,cwd,ref si,out pi));
            process=pi.process; Check(ResumeThread(pi.thread)!=uint.MaxValue);
        } catch { Dispose(); throw; }
        finally {
            if(pi.thread!=IntPtr.Zero) CloseHandle(pi.thread);
            foreach(IntPtr h in new[]{input,output,error}) if(h!=IntPtr.Zero&&h!=new IntPtr(-1)) CloseHandle(h);
            if(attributes!=IntPtr.Zero) { DeleteProcThreadAttributeList(attributes); Marshal.FreeHGlobal(attributes); }
            foreach(IntPtr h in new[]{jobPointer,handlesPointer,limits}) if(h!=IntPtr.Zero) Marshal.FreeHGlobal(h);
        }
    }
    public bool Exited { get { return WaitForSingleObject(process,0)==0; } }
    public uint Active { get { Accounting a; Check(QueryInformationJobObject(job,1,out a,Marshal.SizeOf(typeof(Accounting)),IntPtr.Zero)); return a.active; } }
    public int ExitCode { get { uint code; Check(GetExitCodeProcess(process,out code)); return unchecked((int)code); } }
    public void Kill() { Check(TerminateJobObject(job,1)); }
    public void Dispose() { if(job!=IntPtr.Zero) { CloseHandle(job); job=IntPtr.Zero; } if(process!=IntPtr.Zero) { CloseHandle(process); process=IntPtr.Zero; } }
}
'@
$resolved = (Get-Command $Executable -CommandType Application -ErrorAction Stop).Source
if ($MinimalPath) {
    $env:PATH=Join-Path $env:SystemRoot 'System32'
    foreach ($name in @('cargo','rustc','node','python','python3','npm','uv')) {
        if (Get-Command $name -CommandType Application -ErrorAction SilentlyContinue) { throw 'Developer tool unexpectedly available in minimal PATH.' }
    }
}
$outPath = Join-Path $scratch 'stdout.bin'
$errPath = Join-Path $scratch 'stderr.bin'
$owner = $null
$streams = @()
$clock = [Diagnostics.Stopwatch]::StartNew()
$last = 0.0
$exitCode = 1
try {
    $owner = [CiOwned]::new($resolved, [string[]]$arguments, $Directory, $outPath, $errPath)
    $streams = @([IO.File]::Open($outPath,'Open','Read','ReadWrite'), [IO.File]::Open($errPath,'Open','Read','ReadWrite'))
    $destinations = @([Console]::OpenStandardOutput(), [Console]::OpenStandardError())
    $buffer = [byte[]]::new(4096)
    $total = 0L
    while ($true) {
        if ((Get-Item -LiteralPath $outPath).Length+(Get-Item -LiteralPath $errPath).Length -gt 16777216) { throw 'Captured files exceeded 16 MiB.' }
        for ($index=0; $index -lt 2; $index++) {
            while (($count = $streams[$index].Read($buffer,0,$buffer.Length)) -gt 0) {
                $total += $count
                if ($total -gt 16777216) { throw 'Captured output exceeded 16 MiB.' }
                $destinations[$index].Write($buffer,0,$count)
                $destinations[$index].Flush()
                $last = $clock.Elapsed.TotalSeconds
            }
        }
        if ($owner.Exited) {
            $exitCode = $owner.ExitCode
            if ($owner.Active -gt 0) { $owner.Kill(); throw 'Descendant survived its root process.' }
            for ($index=0; $index -lt 2; $index++) {
                while (($count=$streams[$index].Read($buffer,0,$buffer.Length)) -gt 0) {
                    $total += $count
                    if ($total -gt 16777216) { throw 'Captured output exceeded 16 MiB.' }
                    $destinations[$index].Write($buffer,0,$count)
                    $destinations[$index].Flush()
                }
            }
            break
        }
        if ($clock.Elapsed.TotalSeconds -ge $Wall -or $clock.Elapsed.TotalSeconds-$last -ge $Idle) { throw 'Owned command wall/idle timeout.' }
        # A bounded progress poll, not an unconditional readiness sleep.
        [Threading.Thread]::Sleep(25)
    }
    if ($owner.Active -gt 0) { $owner.Kill(); throw 'Descendant survived its root process.' }
} finally {
    if ($owner) {
        if ($owner.Active -gt 0) { $owner.Kill() }
        $deadline = $clock.Elapsed.TotalSeconds + 5
        while ($owner.Active -gt 0 -and $clock.Elapsed.TotalSeconds -lt $deadline) { [Threading.Thread]::Sleep(25) }
        $remaining = $owner.Active
        $owner.Dispose()
        if ($remaining -gt 0) { throw 'Owned Job cleanup did not reach zero active processes.' }
    }
    foreach ($stream in $streams) { $stream.Dispose() }
    if ($exitCode -eq 0) { [IO.Directory]::Delete($scratch,$true) }
}
exit $exitCode

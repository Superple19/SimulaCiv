param(
    [Parameter(Mandatory = $true)] [string]$PassExe,
    [Parameter(Mandatory = $true)] [string]$TagExe,
    [Parameter(Mandatory = $true)] [string]$HeadExe,
    [string]$Output = "target/m4_02_incremental_diagnosis.tsv"
)

$ErrorActionPreference = "Stop"
$cells = @(
    @{ Id = "m3p-incremental-10k"; RepetitionsHint = 3 },
    @{ Id = "m3p-incremental-100k"; RepetitionsHint = 1 }
)
$builds = @(
    @{ Label = "m4-02-pass"; Exe = $PassExe },
    @{ Label = "v0.5.0-m4"; Exe = $TagExe },
    @{ Label = "head"; Exe = $HeadExe }
)

function Get-HostSample {
    $samples = Get-Counter -Counter '\Processor(_Total)\% Processor Time' -SampleInterval 2 -MaxSamples 5
    $os = Get-CimInstance Win32_OperatingSystem
    [pscustomobject]@{
        utc = [DateTime]::UtcNow.ToString('o')
        total_cpu_samples_percent = @($samples.CounterSamples | ForEach-Object { [Math]::Round($_.CookedValue, 2) })
        free_ram_bytes = [int64]$os.FreePhysicalMemory * 1024
    }
}

function Start-Worker([string]$Executable, [string]$Label, [string]$CellId) {
    $start = [System.Diagnostics.ProcessStartInfo]::new()
    $start.FileName = [System.IO.Path]::GetFullPath($Executable)
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.ArgumentList.Add('--stream')
    $start.ArgumentList.Add($Label)
    $start.ArgumentList.Add($CellId)
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $start
    if (-not $process.Start()) { throw "failed to start $Label/$CellId" }
    $process.PriorityClass = [System.Diagnostics.ProcessPriorityClass]::High
    $process.ProcessorAffinity = [IntPtr]0x555
    $ready = $process.StandardOutput.ReadLine()
    if (-not $ready.StartsWith("READY`t$Label`t$CellId`t")) {
        $err = $process.StandardError.ReadToEnd()
        throw "unexpected worker handshake '$ready' from $Label/$CellId $err"
    }
    $suggested = [int]($ready.Split("`t")[-1])
    [pscustomobject]@{
        Label = $Label
        CellId = $CellId
        Process = $process
        SuggestedRepetitions = $suggested
        ReadyLine = $ready
    }
}

function Stop-Workers($Workers) {
    foreach ($worker in $Workers) {
        if (-not $worker.Process.HasExited) {
            $worker.Process.StandardInput.WriteLine('QUIT')
            $worker.Process.StandardInput.Flush()
            if (-not $worker.Process.WaitForExit(5000)) { $worker.Process.Kill() }
        }
        $worker.Process.Dispose()
    }
}

$outputPath = [System.IO.Path]::GetFullPath($Output)
[System.IO.Directory]::CreateDirectory([System.IO.Path]::GetDirectoryName($outputPath)) | Out-Null
$hostBefore = Get-HostSample
$started = [DateTime]::UtcNow
Write-Output "TIMING RUN START UTC $($started.ToString('o'))"
Write-Output "HOST BEFORE $($hostBefore | ConvertTo-Json -Compress)"
Write-Output "PRIORITY High; affinity=0x555; paired order alternates by sample"

$writer = [System.IO.StreamWriter]::new($outputPath, $false, [System.Text.UTF8Encoding]::new($false))
$writer.WriteLine("kind`tbuild`tcell`tsample`tr`tdirect_ms`tcandidate_ms`tdelta_ms`traw")
$summaries = [System.Collections.Generic.List[string]]::new()
try {
    foreach ($cell in $cells) {
        $workers = [System.Collections.Generic.List[object]]::new()
        try {
            foreach ($build in $builds) {
                $worker = Start-Worker $build.Exe $build.Label $cell.Id
                $workers.Add($worker)
                $writer.WriteLine("ready`t$($worker.Label)`t$($worker.CellId)`t`t$($worker.SuggestedRepetitions)`t`t`t`t$($worker.ReadyLine)")
            }
            $r = ($workers | Measure-Object -Property SuggestedRepetitions -Maximum).Maximum
            foreach ($worker in $workers) {
                $worker.Process.StandardInput.WriteLine("SET_R`t$r")
                $worker.Process.StandardInput.Flush()
            }
            foreach ($worker in $workers) {
                $armed = $worker.Process.StandardOutput.ReadLine()
                if ($armed -ne "ARMED`t$($worker.Label)`t$($worker.CellId)`t$r") {
                    throw "unexpected warmup handshake '$armed'"
                }
                $writer.WriteLine("armed`t$($worker.Label)`t$($worker.CellId)`t`t$r`t`t`t`t$armed")
            }

            for ($sample = 0; $sample -lt 41; $sample++) {
                $order = $workers.ToArray()
                if (($sample % 2) -ne 0) { [Array]::Reverse($order) }
                $pathOrder = if (($sample % 2) -eq 0) { "1" } else { "0" }
                foreach ($worker in $order) {
                    $worker.Process.StandardInput.WriteLine("SAMPLE`t$sample`t$pathOrder")
                    $worker.Process.StandardInput.Flush()
                    $line = $worker.Process.StandardOutput.ReadLine()
                    $fields = $line.Split("`t")
                    if ($fields.Length -ne 8 -or $fields[0] -ne 'SAMPLE') { throw "invalid sample '$line'" }
                    $writer.WriteLine("sample`t$($fields[1])`t$($fields[2])`t$($fields[3])`t$($fields[4])`t$($fields[5])`t$($fields[6])`t$($fields[7])`t$line")
                }
                $writer.Flush()
            }

            foreach ($worker in $workers) {
                $worker.Process.StandardInput.WriteLine('FINISH')
                $worker.Process.StandardInput.Flush()
            }
            foreach ($worker in $workers) {
                $summary = $worker.Process.StandardOutput.ReadLine()
                if (-not $summary.StartsWith("SUMMARY`t$($worker.Label)`t$($worker.CellId)`t")) {
                    throw "invalid summary '$summary'"
                }
                $summaries.Add($summary)
                $writer.WriteLine("summary`t$($worker.Label)`t$($worker.CellId)`t`t$($r)`t`t`t`t$summary")
            }
        }
        finally {
            Stop-Workers $workers
        }
    }
}
finally {
    $writer.Dispose()
}

$hostAfter = Get-HostSample
$ended = [DateTime]::UtcNow
Write-Output "HOST AFTER $($hostAfter | ConvertTo-Json -Compress)"
Write-Output "TIMING RUN END UTC $($ended.ToString('o'))"
Write-Output "OUTPUT $outputPath"
$summaries | ForEach-Object { Write-Output $_ }

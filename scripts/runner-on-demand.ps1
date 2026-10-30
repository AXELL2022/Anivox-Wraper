# Запускает локальный self-hosted GitHub Actions runner по требованию и выключает его,
# когда сборки закончились. Вызывается из .githooks/pre-push.
#
# Логика:
#  * runner уже работает (запущен вручную / службой) -> ничего не делаем и не останавливаем;
#  * иначе запускаем его скрыто, ждём появления и завершения job (Runner.Worker),
#    после паузы GraceSec без новых job останавливаем runner;
#  * если job так и не появился за StartupWaitSec - тоже останавливаем.
# Повторный вызов во время работы лишь продлевает ожидание (файл-триггер).
param(
    [int]$StartupWaitSec = 300,
    [int]$GraceSec = 90,
    [switch]$Child
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$runnerDir = Join-Path $repo 'actions-runner'
$tmpDir = Join-Path $repo 'tmp'
$log = Join-Path $tmpDir 'runner-on-demand.log'
$trigger = Join-Path $tmpDir 'runner-on-demand.trigger'
New-Item -ItemType Directory -Force $tmpDir | Out-Null

# Из хука: перезапускаем себя скрытым процессом и сразу возвращаем управление git.
if (-not $Child) {
    Start-Process powershell.exe -WindowStyle Hidden -ArgumentList @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"",
        '-Child', '-StartupWaitSec', $StartupWaitSec, '-GraceSec', $GraceSec)
    exit 0
}

function Log($m) { Add-Content -Path $log -Value ("{0:yyyy-MM-dd HH:mm:ss} {1}" -f (Get-Date), $m) }
function Get-RunnerProc($name) {
    Get-Process $name -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$runnerDir\*" }
}

# Сигнал "был новый пуш" - продлевает ожидание уже работающего монитора.
Set-Content -Path $trigger -Value (Get-Date -Format o)

$mutex = New-Object System.Threading.Mutex($false, 'Local\AnivoxRunnerOnDemand')
if (-not $mutex.WaitOne(0)) { Log 'monitor already active, trigger refreshed'; exit 0 }

try {
    if (Get-RunnerProc 'Runner.Listener') { Log 'runner already running (not started by us), leaving it alone'; exit 0 }

    $cmd = Start-Process cmd.exe -ArgumentList '/c', 'run.cmd' -WorkingDirectory $runnerDir -WindowStyle Hidden -PassThru
    Log "runner started (cmd pid $($cmd.Id))"

    $lastBusy = Get-Date
    $sawWorker = $false
    while ($true) {
        Start-Sleep -Seconds 5
        if (Get-RunnerProc 'Runner.Worker') {
            if (-not $sawWorker) { Log 'job started' }
            $sawWorker = $true
            $lastBusy = Get-Date
            continue
        }
        $t = (Get-Item $trigger).LastWriteTime
        if ($t -gt $lastBusy) { $lastBusy = $t }
        $limit = if ($sawWorker) { $GraceSec } else { $StartupWaitSec }
        if (((Get-Date) - $lastBusy).TotalSeconds -gt $limit) { break }
    }
    Log ("no active jobs, stopping runner (sawWorker=$sawWorker)")
} finally {
    if ($cmd) {
        & taskkill.exe /PID $cmd.Id /T /F 2>&1 | Out-Null
        Get-RunnerProc 'Runner.Listener' | Stop-Process -Force -ErrorAction SilentlyContinue
        Log 'runner stopped'
    }
    $mutex.ReleaseMutex()
}

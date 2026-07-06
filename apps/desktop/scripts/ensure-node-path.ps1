# npm / vite / tauri 子プロセス向けに Node.js を PATH 先頭へ追加
$nodeDir = "C:\Program Files\nodejs"
if (Test-Path "$nodeDir\node.exe") {
    if (-not ($env:Path -split ';' | Where-Object { $_ -eq $nodeDir })) {
        $env:Path = "$nodeDir;$env:Path"
    }
}

# インストール直後の端末向け: マシン/ユーザー PATH を再読込
$machine = [System.Environment]::GetEnvironmentVariable("Path", "Machine")
$user = [System.Environment]::GetEnvironmentVariable("Path", "User")
if ($machine -and $user) {
    $env:Path = "$machine;$user"
} elseif ($machine) {
    $env:Path = $machine
}

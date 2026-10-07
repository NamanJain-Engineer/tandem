# Tandem shell integration for PowerShell (F-CD-10, ARCHITECTURE §5.8).
# Dot-sourced with `-NoExit -Command ". '<this file>'"`. It wraps `prompt` to
# report the cwd (OSC 7) and prompt boundaries (OSC 133 A, B, D) and marks the
# start of each command (C) from PSReadLine's Enter handler. Every mark carries
# this launch's token, which leaves the environment first so programs started
# from the shell never inherit it. C is written before PSReadLine echoes the
# line break, so it carries `nl=1` and Tandem drops that first line break from
# the command's output.
if ($env:TANDEM_SHELL_TOKEN -and -not $Global:__TandemToken) {
    $Global:__TandemToken = $env:TANDEM_SHELL_TOKEN
    Remove-Item Env:TANDEM_SHELL_TOKEN
    $Global:__TandemPrompt = $function:prompt
    $Global:__TandemLastId = -1
    $Global:__TandemRunning = $false

    function Global:__TandemMark([string] $Body) {
        "$([char]27)]133;$Body;tandem=$($Global:__TandemToken)$([char]7)"
    }

    function Global:prompt {
        $ok = $?
        $code = $Global:LASTEXITCODE
        $out = ""
        if ($Global:__TandemRunning) {
            $exit = if ($ok) { 0 } elseif ($code) { $code } else { 1 }
            $out += __TandemMark "D;$exit"
            $Global:__TandemRunning = $false
        }
        $loc = Get-Location
        if ($loc.Provider.Name -eq "FileSystem") {
            $path = $loc.ProviderPath -replace '\\', '/'
            if ($path -notmatch '^/') { $path = "/$path" }
            $uri = ($path -split '/' | ForEach-Object { [Uri]::EscapeDataString($_) }) -join '/'
            $out += "$([char]27)]7;file://$([Environment]::MachineName)$uri;tandem=$($Global:__TandemToken)$([char]7)"
        }
        $out += __TandemMark "A"
        $out += & $Global:__TandemPrompt
        $out += __TandemMark "B"
        $Global:LASTEXITCODE = $code
        return $out
    }

    if (Get-Module PSReadLine) {
        Set-PSReadLineKeyHandler -Chord Enter -ScriptBlock {
            $line = $null
            $cursor = $null
            [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref] $line, [ref] $cursor)
            if ($line.Trim()) {
                $Global:__TandemRunning = $true
                [Console]::Write((__TandemMark "C;nl=1"))
            }
            [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
        }
    }
}

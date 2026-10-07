# Tandem shell integration for fish (F-CD-10, ARCHITECTURE §5.8). Loaded with
# `fish -l --init-command "source <this file>"`, after the user's config. It
# reports the cwd (OSC 7) and prompt/command boundaries (OSC 133 A, C, D),
# each carrying this launch's token, which leaves the environment first so
# programs started from the shell never inherit it. The prompt function is
# left alone, so fish has no B mark (nothing Tandem does needs it).
if set -q TANDEM_SHELL_TOKEN; and not set -q __tandem_token
    set -g __tandem_token $TANDEM_SHELL_TOKEN
    set -e TANDEM_SHELL_TOKEN
    set -g __tandem_in_command 0

    function __tandem_mark
        printf '\e]133;%s;tandem=%s\a' $argv[1] $__tandem_token
    end

    function __tandem_preexec --on-event fish_preexec
        set -g __tandem_in_command 1
        __tandem_mark C
    end

    function __tandem_postexec --on-event fish_postexec
        set -l st $status
        if test "$__tandem_in_command" = 1
            __tandem_mark "D;$st"
            set -g __tandem_in_command 0
        end
    end

    function __tandem_prompt_start --on-event fish_prompt
        printf '\e]7;file://%s%s;tandem=%s\a' $hostname (string escape --style=url -- $PWD) $__tandem_token
        __tandem_mark A
    end
end

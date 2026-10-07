# Tandem shell integration for bash (F-CD-10, ARCHITECTURE §5.8).
# Started as `bash --rcfile <this file> -i`. It reads the user's startup files
# the way a login shell would (TANDEM_SHELL_LOGIN=1) or ~/.bashrc otherwise,
# then reports the cwd (OSC 7) and prompt/command boundaries (OSC 133 A-D).
# Every mark carries this launch's token, which is moved out of the
# environment first so programs started from the shell never inherit it.

if [ -z "${__tandem_loaded-}" ]; then
__tandem_loaded=1
__tandem_token="${TANDEM_SHELL_TOKEN-}"
unset TANDEM_SHELL_TOKEN

if [ -n "${TANDEM_SHELL_LOGIN-}" ]; then
  unset TANDEM_SHELL_LOGIN
  if [ -r /etc/profile ]; then . /etc/profile; fi
  if [ -r "$HOME/.bash_profile" ]; then . "$HOME/.bash_profile"
  elif [ -r "$HOME/.bash_login" ]; then . "$HOME/.bash_login"
  elif [ -r "$HOME/.profile" ]; then . "$HOME/.profile"
  fi
else
  if [ -r "$HOME/.bashrc" ]; then . "$HOME/.bashrc"; fi
fi

if [ -n "$__tandem_token" ]; then
  __tandem_status=0
  __tandem_in_command=0
  __tandem_ready=0
  __tandem_b='\[\e]133;B;tandem='"$__tandem_token"'\a\]'

  __tandem_mark() {
    builtin printf '\033]133;%s;tandem=%s\007' "$1" "$__tandem_token"
  }

  # Percent-encodes $1 into __tandem_enc (no subshell per prompt).
  __tandem_encode() {
    local LC_ALL=C s="$1" out="" c i
    for (( i = 0; i < ${#s}; i++ )); do
      c="${s:i:1}"
      case "$c" in
        [a-zA-Z0-9/._~-]) out+="$c" ;;
        *) builtin printf -v c '%%%02X' "'$c"; out+="$c" ;;
      esac
    done
    __tandem_enc="$out"
  }

  # First in PROMPT_COMMAND: closes the command that just ran.
  __tandem_precmd() {
    __tandem_status=$?
    __tandem_ready=0
    if [ "$__tandem_in_command" = 1 ]; then
      __tandem_mark "D;$__tandem_status"
      __tandem_in_command=0
    fi
    return "$__tandem_status"
  }

  # Last in PROMPT_COMMAND: the cwd, prompt start, and prompt end in PS1.
  __tandem_prompt() {
    __tandem_encode "$PWD"
    builtin printf '\033]7;file://%s%s;tandem=%s\007' "${HOSTNAME:-localhost}" "$__tandem_enc" "$__tandem_token"
    __tandem_mark "A"
    case "$PS1" in
      *"$__tandem_b"*) ;;
      *) PS1="$PS1$__tandem_b" ;;
    esac
    __tandem_ready=1
    return "$__tandem_status"
  }

  # DEBUG trap: the first command after a prompt starts the command.
  __tandem_preexec() {
    [ "$__tandem_ready" = 1 ] || return 0
    [ -n "${COMP_LINE-}" ] && return 0
    # An empty line runs only PROMPT_COMMAND: no command started.
    case "$BASH_COMMAND" in __tandem_precmd*) return 0 ;; esac
    __tandem_ready=0
    __tandem_in_command=1
    __tandem_mark "C"
  }

  PROMPT_COMMAND="__tandem_precmd${PROMPT_COMMAND:+; $PROMPT_COMMAND}; __tandem_prompt"
  trap '__tandem_preexec' DEBUG
fi
fi

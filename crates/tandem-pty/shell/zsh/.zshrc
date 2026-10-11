# Tandem shell integration for zsh: the user's .zshrc, then the hooks that
# report the cwd (OSC 7) and prompt/command boundaries (OSC 133 A-D), each
# carrying this launch's token (see .zshenv).
ZDOTDIR="${__tandem_zdotdir:-$HOME}"
[[ -r "$ZDOTDIR/.zshrc" ]] && builtin source "$ZDOTDIR/.zshrc"
[[ "$ZDOTDIR" != "${__tandem_zdotdir:-$HOME}" ]] && __tandem_zdotdir="$ZDOTDIR"
ZDOTDIR="$__tandem_shim"

if [[ -n "$__tandem_token" ]]; then
  typeset -gi __tandem_in_command=0 __tandem_status=0
  typeset -g __tandem_b=$'%{\e]133;B;tandem='"$__tandem_token"$'\a%}'

  __tandem_mark() {
    builtin printf '\e]133;%s;tandem=%s\a' "$1" "$__tandem_token"
  }

  # Percent-encodes $1 into __tandem_enc (bytes, not characters).
  __tandem_encode() {
    local LC_ALL=C c s="$1" enc=""
    local -i i
    for (( i = 1; i <= ${#s}; i++ )); do
      c="${s[i]}"
      case "$c" in
        [a-zA-Z0-9/._~-]) enc+="$c" ;;
        *) builtin printf -v c '%%%02X' "'$c"; enc+="$c" ;;
      esac
    done
    typeset -g __tandem_enc="$enc"
  }

  # First precmd hook: the status of the command that just ran.
  __tandem_status_hook() {
    __tandem_status=$?
  }

  # Last precmd hook: close the command, then the cwd and prompt start.
  __tandem_precmd() {
    if (( __tandem_in_command )); then
      __tandem_mark "D;$__tandem_status"
      __tandem_in_command=0
    fi
    __tandem_encode "$PWD"
    builtin printf '\e]7;file://%s%s;tandem=%s\a' "${HOST:-localhost}" "$__tandem_enc" "$__tandem_token"
    __tandem_mark A
    [[ "$PS1" == *"$__tandem_b"* ]] || PS1="$PS1$__tandem_b"
  }

  # $1 is the line as typed (F-EX-02); a leading space keeps it out.
  __tandem_preexec() {
    __tandem_in_command=1
    if [[ -n "$1" && "$1" != " "* ]]; then
      __tandem_encode "$1"
      if (( ${#__tandem_enc} <= 12000 )); then
        __tandem_mark "C;cmdline_url=$__tandem_enc"
        return
      fi
    fi
    __tandem_mark C
  }

  autoload -Uz add-zsh-hook
  precmd_functions=(__tandem_status_hook ${precmd_functions[@]})
  add-zsh-hook precmd __tandem_precmd
  add-zsh-hook preexec __tandem_preexec
fi

# Not a login shell: startup ends here.
if [[ ! -o login ]]; then
  if [[ -n "$__tandem_zdotdir" ]]; then ZDOTDIR="$__tandem_zdotdir"; else unset ZDOTDIR; fi
  unset __tandem_shim __tandem_zdotdir
fi

# Tandem shell integration for zsh (F-CD-10, ARCHITECTURE §5.8).
# ZDOTDIR points at this folder. Each file here runs the user's own file from
# their real ZDOTDIR (TANDEM_REAL_ZDOTDIR, else $HOME) at the top level, so
# their declarations stay global; .zshrc adds the hooks; ZDOTDIR goes back to
# the user's value when startup ends, so shells started later read their own.
typeset -g __tandem_shim="$ZDOTDIR"
typeset -g __tandem_token="${TANDEM_SHELL_TOKEN-}"
unset TANDEM_SHELL_TOKEN
typeset -g __tandem_zdotdir="${TANDEM_REAL_ZDOTDIR-}"
unset TANDEM_REAL_ZDOTDIR

ZDOTDIR="${__tandem_zdotdir:-$HOME}"
[[ -r "$ZDOTDIR/.zshenv" ]] && builtin source "$ZDOTDIR/.zshenv"
[[ "$ZDOTDIR" != "${__tandem_zdotdir:-$HOME}" ]] && __tandem_zdotdir="$ZDOTDIR"
ZDOTDIR="$__tandem_shim"

# A shell that reads nothing else (scripts, `zsh -c`): hand ZDOTDIR back now.
if [[ ! -o interactive && ! -o login ]]; then
  if [[ -n "$__tandem_zdotdir" ]]; then ZDOTDIR="$__tandem_zdotdir"; else unset ZDOTDIR; fi
  unset __tandem_shim __tandem_zdotdir __tandem_token
fi

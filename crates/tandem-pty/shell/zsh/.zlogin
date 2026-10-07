# Tandem shell integration for zsh: the user's .zlogin, the last startup file
# of a login shell, then ZDOTDIR goes back to the user's value.
ZDOTDIR="${__tandem_zdotdir:-$HOME}"
[[ -r "$ZDOTDIR/.zlogin" ]] && builtin source "$ZDOTDIR/.zlogin"
[[ "$ZDOTDIR" != "${__tandem_zdotdir:-$HOME}" ]] && __tandem_zdotdir="$ZDOTDIR"
if [[ -n "$__tandem_zdotdir" ]]; then ZDOTDIR="$__tandem_zdotdir"; else unset ZDOTDIR; fi
unset __tandem_shim __tandem_zdotdir

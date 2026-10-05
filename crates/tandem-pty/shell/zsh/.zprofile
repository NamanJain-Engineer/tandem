# Tandem shell integration for zsh: the user's .zprofile (see .zshenv).
ZDOTDIR="${__tandem_zdotdir:-$HOME}"
[[ -r "$ZDOTDIR/.zprofile" ]] && builtin source "$ZDOTDIR/.zprofile"
[[ "$ZDOTDIR" != "${__tandem_zdotdir:-$HOME}" ]] && __tandem_zdotdir="$ZDOTDIR"
ZDOTDIR="$__tandem_shim"

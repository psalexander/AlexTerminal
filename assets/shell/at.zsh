# AlexTerminal shell integration for zsh.
# Reports cwd (OSC 7), prompt/command marks (OSC 133) and the command line (OSC 7770).
[[ -n "$_ALEXTERM_ZSH_LOADED" ]] && return
typeset -g _ALEXTERM_ZSH_LOADED=1
zmodload zsh/parameter 2>/dev/null
autoload -Uz add-zsh-hook

typeset -g _at_first=1
typeset -g _at_running=

_at_urlencode() {
  local s="$1" out="" c i
  for (( i = 1; i <= ${#s}; i++ )); do
    c="${s[i]}"
    case "$c" in
      [a-zA-Z0-9/._~-]) out+="$c" ;;
      *) out+=$(printf '%%%02X' "'$c") ;;
    esac
  done
  print -rn -- "$out"
}

_at_precmd() {
  local ret=$?
  if [[ -n "$_at_running" ]]; then
    printf '\e]133;D;%s\a' "$ret"
    _at_running=
  fi
  if [[ -n "$_at_first" ]]; then
    _at_first=
    if [[ -n "$ALEXTERM_HISTFILE" ]]; then
      fc -p "$ALEXTERM_HISTFILE" "${HISTSIZE:-10000}" "${SAVEHIST:-10000}"
      setopt INC_APPEND_HISTORY 2>/dev/null
    fi
  fi
  printf '\e]7;file://%s%s\a' "${HOST}" "$(_at_urlencode "$PWD")"
  printf '\e]133;A\a'
}

_at_preexec() {
  _at_running=1
  printf '\e]7770;cmd=%s\a' "$(print -rn -- "$1" | base64 | tr -d '\n')"
  printf '\e]133;C\a'
}

add-zsh-hook precmd _at_precmd
add-zsh-hook preexec _at_preexec

if [[ -n "$ALEXTERM_WRAP_SSH" ]]; then
  _at_ssh_host() {
    local a skip=
    for a in "$@"; do
      if [[ -n "$skip" ]]; then skip=; continue; fi
      case "$a" in
        -[BbcDEeFIiJLlmOoPpQRSWw]) skip=1 ;;
        -*) ;;
        *) print -rn -- "${a##*@}"; return ;;
      esac
    done
  }
  ssh() {
    local h; h=$(_at_ssh_host "$@")
    printf '\e]7770;remote=%s\a' "$h"
    command ssh "$@"
    local r=$?
    printf '\e]7770;remote=\a'
    return $r
  }
fi

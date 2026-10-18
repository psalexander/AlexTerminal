# AlexTerminal shell integration for bash (>= 4.4).
[[ -n "$_ALEXTERM_BASH_LOADED" ]] && return
_ALEXTERM_BASH_LOADED=1

if [[ -n "$ALEXTERM_HISTFILE" ]]; then
  HISTFILE="$ALEXTERM_HISTFILE"
  history -c
  history -r
  shopt -s histappend
fi

_at_running=
_at_in_prompt=

_at_preexec() {
  [[ -n "$COMP_LINE" || -n "$_at_in_prompt" || -n "$_at_running" ]] && return
  _at_running=1
  local c
  c=$(HISTTIMEFORMAT= builtin history 1 2>/dev/null | sed 's/^ *[0-9]* *//')
  [[ -z "$c" ]] && c="$BASH_COMMAND"
  printf '\e]7770;cmd=%s\a' "$(printf '%s' "$c" | base64 | tr -d '\n')"
  printf '\e]133;C\a'
}

_at_precmd() {
  local ret=$?
  _at_in_prompt=1
  if [[ -n "$_at_running" ]]; then
    printf '\e]133;D;%s\a' "$ret"
  fi
  _at_running=
  [[ -n "$ALEXTERM_HISTFILE" ]] && history -a
  printf '\e]7;file://%s%s\a' "$HOSTNAME" "${PWD// /%20}"
  printf '\e]133;A\a'
  return $ret
}

_at_prompt_done() {
  _at_in_prompt=
}

trap '_at_preexec' DEBUG
PROMPT_COMMAND="_at_precmd${PROMPT_COMMAND:+; $PROMPT_COMMAND}; _at_prompt_done"

if [[ -n "$ALEXTERM_WRAP_SSH" ]]; then
  _at_ssh_host() {
    local a skip=
    for a in "$@"; do
      if [[ -n "$skip" ]]; then skip=; continue; fi
      case "$a" in
        -[BbcDEeFIiJLlmOoPpQRSWw]) skip=1 ;;
        -*) ;;
        *) printf '%s' "${a##*@}"; return ;;
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

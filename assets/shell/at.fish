# AlexTerminal shell integration for fish (loaded from vendor_conf.d).
if set -q _ALEXTERM_FISH_LOADED
    exit
end
set -g _ALEXTERM_FISH_LOADED 1

if set -q ALEXTERM_HISTORY_NAME
    set -g fish_history $ALEXTERM_HISTORY_NAME
end

function __at_prompt --on-event fish_prompt
    if set -q __at_running
        printf '\e]133;D;%s\a' $__at_status
        set -e __at_running
    end
    printf '\e]7;file://%s%s\a' (hostname) (string escape --style=url -- $PWD | string replace -a '%2F' '/')
    printf '\e]133;A\a'
end

function __at_preexec --on-event fish_preexec
    set -g __at_running 1
    printf '\e]7770;cmd=%s\a' (printf '%s' "$argv" | base64 | tr -d '\n')
    printf '\e]133;C\a'
end

function __at_postexec --on-event fish_postexec
    set -g __at_status $status
end

if set -q ALEXTERM_WRAP_SSH
    function ssh --wraps ssh
        set -l h
        set -l skip 0
        for a in $argv
            if test $skip -eq 1
                set skip 0
                continue
            end
            switch $a
                case '-B' '-b' '-c' '-D' '-E' '-e' '-F' '-I' '-i' '-J' '-L' '-l' '-m' '-O' '-o' '-P' '-p' '-Q' '-R' '-S' '-W' '-w'
                    set skip 1
                case '-*'
                case '*'
                    set h (string replace -r '.*@' '' -- $a)
                    break
            end
        end
        printf '\e]7770;remote=%s\a' $h
        command ssh $argv
        set -l r $status
        printf '\e]7770;remote=\a'
        return $r
    end
end

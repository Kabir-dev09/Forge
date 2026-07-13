if [ -z "$FORGE_INTEGRATION_ZSH" ]; then
    export FORGE_INTEGRATION_ZSH=1
    
    forge_preexec() {
        printf "\e]133;C;%s\a" "$1"
    }

    forge_precmd() {
        local exit_code=$?
        printf "\e]133;D;%s\a" "$exit_code"
        printf "\e]133;A\a"
        printf "\e]7;file://%s%s\a" "${HOST:-localhost}" "$PWD"
    }

    autoload -Uz add-zsh-hook
    add-zsh-hook preexec forge_preexec
    add-zsh-hook precmd forge_precmd
fi

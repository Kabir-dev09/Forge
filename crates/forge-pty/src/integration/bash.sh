if [ -z "$FORGE_INTEGRATION_BASH" ]; then
    export FORGE_INTEGRATION_BASH=1
    PS0="\e]133;C\a"
    forge_precmd() {
        local exit_code=$?
        printf "\e]133;D;%s\a" "$exit_code"
        printf "\e]133;A\a"
        printf "\e]7;file://%s%s\a" "${HOSTNAME:-localhost}" "$PWD"
    }
    PROMPT_COMMAND="forge_precmd${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
fi

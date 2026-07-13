if not set -q FORGE_INTEGRATION_FISH
    set -gx FORGE_INTEGRATION_FISH 1

    function forge_preexec --on-event fish_preexec
        printf "\e]133;C;%s\a" "$argv"
    end

    function forge_postexec --on-event fish_postexec
        printf "\e]133;D;%s\a" $status
    end

    function forge_prompt --on-event fish_prompt
        printf "\e]133;A\a"
        printf "\e]7;file://%s%s\a" $hostname $PWD
    end
end

if not ("FORGE_INTEGRATION_NU" in $env) {
    $env.FORGE_INTEGRATION_NU = 1
    
    let current_pre_prompt = if ("config" in $env) and ("hooks" in $env.config) and ("pre_prompt" in $env.config.hooks) {
        $env.config.hooks.pre_prompt
    } else {
        []
    }
    
    let forge_pre_prompt = {||
        let exit_code = $env.LAST_EXIT_CODE
        print -n $"\e]133;D;($exit_code)\a"
        print -n $"\e]133;A\a"
        print -n $"\e]7;file://localhost($env.PWD)\a"
    }
    
    let current_pre_exec = if ("config" in $env) and ("hooks" in $env.config) and ("pre_execution" in $env.config.hooks) {
        $env.config.hooks.pre_execution
    } else {
        []
    }
    
    let forge_pre_exec = {||
        print -n $"\e]133;C\a"
    }
    
    $env.config = ($env.config | upsert hooks.pre_prompt ($current_pre_prompt | append $forge_pre_prompt) | upsert hooks.pre_execution ($current_pre_exec | append $forge_pre_exec))
}

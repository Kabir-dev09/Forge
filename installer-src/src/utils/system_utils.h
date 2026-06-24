#pragma once
#include <string>

namespace utils {

class SystemUtils {
public:
    // Gets the home directory of the actual user, even if running under sudo or pkexec
    static std::string get_real_user_home();

    // Gets the username of the actual user, even if running under sudo or pkexec
    static std::string get_real_user_name();
    
    // Runs chown on a given path for the real user (no-op if not running as root or if real user not found)
    // recursive: if true, adds -R
    // symlink_only: if true, adds -h
    static void chown_to_real_user(const std::string& path, bool recursive = false, bool symlink_only = false);
    
    // Runs a command using sudo -S, feeding the password securely via stdin using popen
    static int run_elevated(const std::string& command, const std::string& password);
};

} // namespace utils

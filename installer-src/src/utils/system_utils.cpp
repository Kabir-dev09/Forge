#include "system_utils.h"
#include <cstdlib>
#include <pwd.h>
#include <unistd.h>

namespace utils {

std::string SystemUtils::get_real_user_name() {
    const char* sudo_user = std::getenv("SUDO_USER");
    if (sudo_user) return sudo_user;
    
    const char* pkexec_uid = std::getenv("PKEXEC_UID");
    if (pkexec_uid) {
        uid_t uid = std::stoi(pkexec_uid);
        struct passwd* pw = getpwuid(uid);
        if (pw) return pw->pw_name;
    }
    return "";
}

std::string SystemUtils::get_real_user_home() {
    std::string real_user = get_real_user_name();
    if (!real_user.empty()) {
        struct passwd* pw = getpwnam(real_user.c_str());
        if (pw) return pw->pw_dir;
    }
    
    const char* home = std::getenv("HOME");
    if (home) return home;
    
    return std::string(getpwuid(getuid())->pw_dir);
}

void SystemUtils::chown_to_real_user(const std::string& path, bool recursive, bool symlink_only) {
    std::string real_user = get_real_user_name();
    if (real_user.empty()) return; // Not running via sudo/pkexec
    
    std::string cmd = "chown ";
    if (recursive) cmd += "-R ";
    if (symlink_only) cmd += "-h ";
    
    cmd += real_user + ":" + real_user + " '" + path + "'";
    std::system(cmd.c_str());
}

int SystemUtils::run_elevated(const std::string& command, const std::string& password) {
    if (password.empty()) {
        return std::system(command.c_str());
    }
    
    // sudo -S reads password from stdin. -p '' suppresses the prompt text.
    std::string full_cmd = "sudo -S -p '' " + command;
    FILE* pipe = popen(full_cmd.c_str(), "w");
    if (!pipe) return -1;
    
    fprintf(pipe, "%s\n", password.c_str());
    fflush(pipe);
    
    return pclose(pipe);
}

} // namespace utils

#pragma once
#include <string>

namespace core {

enum class InstallScope {
    NONE,
    GLOBAL,
    CURRENT_USER
};

struct InstallerState {
    bool is_installed = false;
    InstallScope scope = InstallScope::NONE;
    std::string installed_version;
    std::string sudo_password;
};

class StateManager {
public:
    // Reads the system and user directories to determine if Forge is installed
    static InstallerState get_current_state();
    
    // Saves the installation state to the correct config directory.
    // sudo_password is required for global installs (writing to /etc/forge/).
    static bool save_state(const InstallerState& state, const std::string& sudo_password = "");
    
    // Removes the state file during uninstallation
    static bool remove_state(InstallScope scope);
    
private:
    static std::string get_global_state_path();
    static std::string get_user_state_path();
};

} // namespace core

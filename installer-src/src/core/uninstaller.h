#pragma once
#include <string>
#include "../core/installer_state.h"

namespace core {

class Uninstaller {
public:
    // Removes the Forge binary, symlink, and state file.
    // sudo_password is required for global uninstalls.
    // Returns true on success.
    static bool uninstall(const InstallerState& state, const std::string& sudo_password = "");
};

} // namespace core

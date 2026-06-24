#include "uninstaller.h"
#include "installer_state.h"
#include "../utils/system_utils.h"
#include <filesystem>
#include <iostream>

namespace core {

bool Uninstaller::uninstall(const InstallerState& state, const std::string& sudo_password) {
    std::error_code ec;
    bool success = true;

    if (state.scope == InstallScope::GLOBAL) {
        // --- Global uninstall: needs sudo ---
        // Remove symlink
        int r1 = utils::SystemUtils::run_elevated("rm -f '/usr/local/bin/forge'", sudo_password);
        if (r1 != 0) {
            std::cerr << "Warning: could not remove /usr/local/bin/forge" << std::endl;
            success = false;
        }

        // Remove the Forge binary directory
        int r2 = utils::SystemUtils::run_elevated("rm -rf '/opt/Forge'", sudo_password);
        if (r2 != 0) {
            std::cerr << "Warning: could not remove /opt/Forge" << std::endl;
            success = false;
        }

        // Remove state file directory
        int r3 = utils::SystemUtils::run_elevated("rm -rf '/etc/forge'", sudo_password);
        if (r3 != 0) {
            std::cerr << "Warning: could not remove /etc/forge" << std::endl;
            success = false;
        }

    } else if (state.scope == InstallScope::CURRENT_USER) {
        // --- User uninstall: plain filesystem ---
        std::string home = utils::SystemUtils::get_real_user_home();

        std::string symlink_path = home + "/.local/bin/forge";
        std::string binary_dir   = home + "/.local/share/Forge";
        std::string state_dir    = home + "/.config/forge";

        // Remove symlink
        if (std::filesystem::exists(symlink_path, ec) || std::filesystem::is_symlink(symlink_path, ec)) {
            std::filesystem::remove(symlink_path, ec);
            if (ec) {
                std::cerr << "Warning: could not remove symlink: " << ec.message() << std::endl;
                success = false;
            }
        }

        // Remove the Forge binary directory
        std::filesystem::remove_all(binary_dir, ec);
        if (ec) {
            std::cerr << "Warning: could not remove " << binary_dir << ": " << ec.message() << std::endl;
            success = false;
        }

        // Remove the state directory
        std::filesystem::remove_all(state_dir, ec);
        if (ec) {
            std::cerr << "Warning: could not remove " << state_dir << ": " << ec.message() << std::endl;
            success = false;
        }
    }

    return success;
}

} // namespace core

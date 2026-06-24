#include "installer_state.h"
#include "../utils/system_utils.h"
#include <nlohmann/json.hpp>
#include <fstream>
#include <filesystem>
#include <cstdlib>
#include <sstream>

namespace core {

std::string StateManager::get_global_state_path() {
    return "/etc/forge/installer_state.json";
}

std::string StateManager::get_user_state_path() {
    std::string home = utils::SystemUtils::get_real_user_home();
    return home + "/.config/forge/installer_state.json";
}

InstallerState StateManager::get_current_state() {
    InstallerState state;
    std::error_code ec;
    
    // Check global first
    std::string global_path = get_global_state_path();
    if (std::filesystem::exists(global_path, ec)) {
        std::ifstream in(global_path);
        if (in.is_open()) {
            try {
                auto j = nlohmann::json::parse(in);
                state.is_installed = true;
                state.scope = InstallScope::GLOBAL;
                state.installed_version = j.value("installed_version", "unknown");
                return state;
            } catch (...) {}
        }
    }
    
    // Check user scope
    std::string user_path = get_user_state_path();
    if (std::filesystem::exists(user_path, ec)) {
        std::ifstream in(user_path);
        if (in.is_open()) {
            try {
                auto j = nlohmann::json::parse(in);
                state.is_installed = true;
                state.scope = InstallScope::CURRENT_USER;
                state.installed_version = j.value("installed_version", "unknown");
                return state;
            } catch (...) {}
        }
    }
    
    // Fallback detection via binaries if json is missing
    if (std::filesystem::exists("/usr/local/bin/forge", ec)) {
        state.is_installed = true;
        state.scope = InstallScope::GLOBAL;
        state.installed_version = "unknown";
    } else if (std::filesystem::exists(utils::SystemUtils::get_real_user_home() + "/.local/bin/forge", ec)) {
        state.is_installed = true;
        state.scope = InstallScope::CURRENT_USER;
        state.installed_version = "unknown";
    }
    
    return state;
}

bool StateManager::save_state(const InstallerState& state, const std::string& sudo_password) {
    if (state.scope == InstallScope::NONE) return false;
    
    nlohmann::json j;
    j["installed_version"] = state.installed_version;
    j["scope"] = (state.scope == InstallScope::GLOBAL) ? "global" : "current_user";
    std::string json_str = j.dump(4);
    
    if (state.scope == InstallScope::GLOBAL) {
        // Write JSON to a temp file first, then sudo mv it into /etc/forge/
        std::string tmp_path = "/tmp/forge_installer_state.json";
        std::ofstream tmp_out(tmp_path);
        if (!tmp_out.is_open()) return false;
        tmp_out << json_str;
        tmp_out.close();
        
        std::string global_dir = "/etc/forge";
        utils::SystemUtils::run_elevated("mkdir -p '" + global_dir + "'", sudo_password);
        
        int res = utils::SystemUtils::run_elevated(
            "mv '" + tmp_path + "' '" + get_global_state_path() + "'", sudo_password);
        return (res == 0);
    } else {
        // Current user: write directly, no elevation needed
        std::string path = get_user_state_path();
        std::error_code ec;
        std::string parent_dir = std::filesystem::path(path).parent_path().string();
        
        bool dir_created = false;
        if (!std::filesystem::exists(parent_dir, ec)) {
            std::filesystem::create_directories(parent_dir, ec);
            dir_created = true;
        }
        if (dir_created) {
            utils::SystemUtils::chown_to_real_user(parent_dir, true);
        }
        
        std::ofstream out(path);
        if (!out.is_open()) return false;
        out << json_str;
        out.close();
        
        utils::SystemUtils::chown_to_real_user(path, false);
        return true;
    }
}

bool StateManager::remove_state(InstallScope scope) {
    std::string path = (scope == InstallScope::GLOBAL) ? get_global_state_path() : get_user_state_path();
    std::error_code ec;
    if (std::filesystem::exists(path, ec)) {
        return std::filesystem::remove(path, ec);
    }
    return true;
}

} // namespace core

#pragma once
#include <string>
#include <vector>

namespace core {

enum class PackageManagerType {
    APT,
    PACMAN,
    DNF,
    UNKNOWN
};

class PackageManager {
public:
    // Detects which package manager is available on the system
    static PackageManagerType detect_package_manager();
    
    // Returns the human-readable name of the package manager
    static std::string get_package_manager_name(PackageManagerType type);

    // Installs the given dependencies using the detected package manager.
    // Uses sudo internally if a sudo_password is provided.
    static bool install_dependencies(const std::vector<std::string>& dependencies, const std::string& sudo_password = "");

    // Maps a generic dependency name to a package-manager specific name
    static std::vector<std::string> map_dependencies(
        PackageManagerType type, 
        const std::vector<std::string>& generic_deps
    );

    // Get build dependencies from the plan
    static std::vector<std::string> get_build_dependencies();
    
    // Get runtime dependencies from the plan
    static std::vector<std::string> get_runtime_dependencies();

    // Generates the command to install the given packages
    static std::string generate_install_command(PackageManagerType type, const std::vector<std::string>& packages);
};

} // namespace core

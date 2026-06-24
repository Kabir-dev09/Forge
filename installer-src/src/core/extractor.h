#pragma once
#include <string>

namespace core {

class Extractor {
public:
    // Extracts the downloaded binary tar.gz and installs it.
    // archive_path: path to the .tar.gz file (e.g. /tmp/forge-binary-v1.0.0.tar.gz)
    // version_tag: version string (e.g. "v1.0.0")
    // is_global: true if installing system-wide, false for current user
    // sudo_password: password for elevated commands (only needed for global installs)
    // opt_base_dir: base directory for the Forge folder (default /opt)
    // global_bin_dir: base directory for global symlinks (default /usr/local/bin)
    // Returns true on success.
    static bool extract_and_install_binary(
        const std::string& archive_path, 
        const std::string& version_tag, 
        bool is_global,
        const std::string& sudo_password = "",
        const std::string& opt_base_dir = "/opt",
        const std::string& global_bin_dir = "/usr/local/bin"
    );
};

} // namespace core

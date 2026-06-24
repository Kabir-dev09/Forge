#include "extractor.h"
#include "../utils/system_utils.h"
#include <cstdlib>
#include <iostream>
#include <filesystem>
#include <unistd.h>

namespace core {

bool Extractor::extract_and_install_binary(
    const std::string& archive_path, 
    const std::string& version_tag, 
    bool is_global,
    const std::string& sudo_password,
    const std::string& opt_base_dir,
    const std::string& global_bin_dir) 
{
    std::string tmp_dir = "/tmp/forge_extract_" + version_tag;
    std::error_code ec;
    std::filesystem::remove_all(tmp_dir, ec);
    std::filesystem::create_directories(tmp_dir, ec);

    // Extract archive (no privileges needed — writing to /tmp)
    std::string tar_cmd = "tar -xzf '" + archive_path + "' -C '" + tmp_dir + "'";
    if (std::system(tar_cmd.c_str()) != 0) {
        std::cerr << "Extraction failed using command: " << tar_cmd << std::endl;
        return false;
    }

    std::string extracted_binary = tmp_dir + "/forge-binary-" + version_tag + "/forge";
    if (!std::filesystem::exists(extracted_binary, ec)) {
        std::cerr << "Could not find 'forge' inside the extracted archive at: " << extracted_binary << std::endl;
        return false;
    }

    if (is_global) {
        // --- Global install: all writes need sudo ---
        std::string opt_dir = opt_base_dir + "/Forge";
        std::string dest_binary = opt_dir + "/forge";
        std::string symlink_path = global_bin_dir + "/forge";

        // Create destination directory
        if (utils::SystemUtils::run_elevated("mkdir -p '" + opt_dir + "'", sudo_password) != 0) {
            std::cerr << "Failed to create directory: " << opt_dir << std::endl;
            return false;
        }

        // Copy binary
        if (utils::SystemUtils::run_elevated("cp '" + extracted_binary + "' '" + dest_binary + "'", sudo_password) != 0) {
            std::cerr << "Failed to copy binary to: " << dest_binary << std::endl;
            return false;
        }

        // Set permissions
        utils::SystemUtils::run_elevated("chmod 755 '" + dest_binary + "'", sudo_password);

        // Remove old symlink if it exists, then create new one
        utils::SystemUtils::run_elevated("rm -f '" + symlink_path + "'", sudo_password);
        if (utils::SystemUtils::run_elevated("ln -s '" + dest_binary + "' '" + symlink_path + "'", sudo_password) != 0) {
            std::cerr << "Failed to create symlink at: " << symlink_path << std::endl;
            return false;
        }

    } else {
        // --- User install: writes to ~/.local, no sudo needed ---
        std::string home = utils::SystemUtils::get_real_user_home();
        std::string opt_dir = home + "/.local/share/Forge";
        std::string dest_binary = opt_dir + "/forge";
        std::string local_bin = home + "/.local/bin";
        std::string symlink_path = local_bin + "/forge";

        std::filesystem::create_directories(opt_dir, ec);
        if (ec) {
            std::cerr << "Failed to create directory: " << opt_dir << ": " << ec.message() << std::endl;
            return false;
        }

        // Copy binary
        std::filesystem::copy_file(extracted_binary, dest_binary, std::filesystem::copy_options::overwrite_existing, ec);
        if (ec) {
            std::cerr << "Failed to copy binary to " << dest_binary << ": " << ec.message() << std::endl;
            return false;
        }

        // Set permissions
        std::filesystem::permissions(dest_binary, 
            std::filesystem::perms::owner_all |
            std::filesystem::perms::group_read | std::filesystem::perms::group_exec |
            std::filesystem::perms::others_read | std::filesystem::perms::others_exec,
            ec);

        // Create ~/.local/bin if it doesn't exist
        bool bin_created = false;
        if (!std::filesystem::exists(local_bin, ec)) {
            std::filesystem::create_directories(local_bin, ec);
            bin_created = true;
        }
        if (bin_created) {
            utils::SystemUtils::chown_to_real_user(local_bin, false);
        }

        // Remove old symlink, create new one
        if (std::filesystem::exists(symlink_path, ec) || std::filesystem::is_symlink(symlink_path, ec)) {
            std::filesystem::remove(symlink_path, ec);
        }
        std::filesystem::create_symlink(dest_binary, symlink_path, ec);
        if (ec) {
            std::cerr << "Failed to create symlink at " << symlink_path << ": " << ec.message() << std::endl;
            return false;
        }

        utils::SystemUtils::chown_to_real_user(symlink_path, false, true);
    }

    std::filesystem::remove_all(tmp_dir, ec);
    return true;
}

} // namespace core

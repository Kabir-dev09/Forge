#include <catch2/catch_test_macros.hpp>
#include "core/package_manager.h"
#include "core/network_client.h"

TEST_CASE("PackageManager Detect and Map", "[package_manager]") {
    using namespace core;
    
    // Test mapping for APT
    auto deps_apt = PackageManager::map_dependencies(PackageManagerType::APT, {"cmake", "rust"});
    REQUIRE(deps_apt.size() == 3); // cmake, rustc, cargo
    
    // Test mapping for PACMAN
    auto deps_pacman = PackageManager::map_dependencies(PackageManagerType::PACMAN, {"cmake", "rust"});
    REQUIRE(deps_pacman.size() == 2); // cmake, rust
    
    // Test command generation
    std::string cmd = PackageManager::generate_install_command(PackageManagerType::APT, deps_apt);
    REQUIRE(cmd == "apt-get install -y cmake rustc cargo");
}

TEST_CASE("NetworkClient Fetch Latest Release", "[network]") {
    using namespace core;
    
    ReleaseInfo release;
    REQUIRE_NOTHROW(release = NetworkClient::get_latest_release());
    REQUIRE(!release.version_tag.empty());
    REQUIRE(release.binary_url.find("forge-binary-") != std::string::npos);
    REQUIRE(release.source_url.find("forge-source-") != std::string::npos);
}

#include "core/extractor.h"
#include <fstream>
#include <filesystem>

TEST_CASE("Extractor Install Binary", "[extractor]") {
    using namespace core;
    
    // Setup dummy tarball
    std::filesystem::create_directories("/tmp/test_forge_env/forge-binary-v1.0.0");
    std::ofstream dummy_bin("/tmp/test_forge_env/forge-binary-v1.0.0/forge");
    dummy_bin << "dummy binary data";
    dummy_bin.close();
    
    std::system("tar -czf /tmp/test_forge_env/test-archive.tar.gz -C /tmp/test_forge_env forge-binary-v1.0.0");
    
    std::string test_opt = "/tmp/test_forge_opt";
    std::string test_bin = "/tmp/test_forge_bin";
    std::filesystem::create_directories(test_bin);
    
    bool result = Extractor::extract_and_install_binary(
        "/tmp/test_forge_env/test-archive.tar.gz",
        "v1.0.0",
        true, // is_global
        test_opt,
        test_bin
    );
    
    REQUIRE(result == true);
    REQUIRE(std::filesystem::exists(test_opt + "/Forge/forge"));
    REQUIRE(std::filesystem::exists(test_bin + "/forge"));
    REQUIRE(std::filesystem::is_symlink(test_bin + "/forge"));
    
    // Cleanup
    std::filesystem::remove_all("/tmp/test_forge_env");
    std::filesystem::remove_all(test_opt);
    std::filesystem::remove_all(test_bin);
}

#include "core/installer_state.h"

TEST_CASE("StateManager State Persistence", "[state]") {
    using namespace core;
    
    // Test that saving works
    InstallerState state;
    state.is_installed = true;
    state.scope = InstallScope::CURRENT_USER;
    state.installed_version = "v1.2.3";
    
    REQUIRE(StateManager::save_state(state) == true);
    
    // Test that reading works
    InstallerState read_state = StateManager::get_current_state();
    REQUIRE(read_state.is_installed == true);
    REQUIRE(read_state.installed_version == "v1.2.3");
    REQUIRE(read_state.scope == InstallScope::CURRENT_USER);
    
    // Clean up
    REQUIRE(StateManager::remove_state(InstallScope::CURRENT_USER) == true);
}

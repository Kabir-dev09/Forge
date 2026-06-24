#include "package_manager.h"
#include "../utils/system_utils.h"
#include <iostream>
#include <filesystem>
#include <algorithm>

namespace core {

PackageManagerType PackageManager::detect_package_manager() {
    if (std::filesystem::exists("/usr/bin/apt") || std::filesystem::exists("/usr/bin/apt-get")) {
        return PackageManagerType::APT;
    }
    if (std::filesystem::exists("/usr/bin/pacman")) {
        return PackageManagerType::PACMAN;
    }
    if (std::filesystem::exists("/usr/bin/dnf")) {
        return PackageManagerType::DNF;
    }
    return PackageManagerType::UNKNOWN;
}

std::string PackageManager::get_package_manager_name(PackageManagerType type) {
    switch (type) {
        case PackageManagerType::APT: return "APT (Debian/Ubuntu)";
        case PackageManagerType::PACMAN: return "Pacman (Arch Linux)";
        case PackageManagerType::DNF: return "DNF (Fedora)";
        default: return "Unknown";
    }
}

std::vector<std::string> PackageManager::get_build_dependencies() {
    return {
        "rust", "c-compiler", "cmake", "pkg-config", "shaderc", 
        "vulkan-headers", "wayland-protocols", "wayland-client-dev", 
        "xkbcommon-dev", "luajit-dev"
    };
}

std::vector<std::string> PackageManager::get_runtime_dependencies() {
    return {
        "vulkan-icd-loader", "wayland-client", "xkbcommon", 
        "libffi", "luajit", "wayland-compositor"
    };
}

std::vector<std::string> PackageManager::map_dependencies(
    PackageManagerType type, 
    const std::vector<std::string>& generic_deps) {
    
    std::vector<std::string> mapped;
    
    for (const auto& dep : generic_deps) {
        if (type == PackageManagerType::APT) {
            if (dep == "rust") { mapped.push_back("rustc"); mapped.push_back("cargo"); }
            else if (dep == "c-compiler") mapped.push_back("build-essential");
            else if (dep == "cmake") mapped.push_back("cmake");
            else if (dep == "pkg-config") mapped.push_back("pkg-config");
            else if (dep == "shaderc") mapped.push_back("glslc");
            else if (dep == "vulkan-headers") mapped.push_back("libvulkan-dev");
            else if (dep == "wayland-protocols") mapped.push_back("wayland-protocols");
            else if (dep == "wayland-client-dev") mapped.push_back("libwayland-dev");
            else if (dep == "xkbcommon-dev") mapped.push_back("libxkbcommon-dev");
            else if (dep == "luajit-dev") mapped.push_back("libluajit-5.1-dev");
            else if (dep == "vulkan-icd-loader") mapped.push_back("vulkan-tools");
            else if (dep == "wayland-client") mapped.push_back("libwayland-client0");
            else if (dep == "xkbcommon") mapped.push_back("libxkbcommon0");
            else if (dep == "libffi") mapped.push_back("libffi-dev");
            else if (dep == "luajit") mapped.push_back("luajit");
        }
        else if (type == PackageManagerType::PACMAN) {
            if (dep == "rust") mapped.push_back("rust");
            else if (dep == "c-compiler") mapped.push_back("base-devel");
            else if (dep == "cmake") mapped.push_back("cmake");
            else if (dep == "pkg-config") mapped.push_back("pkgconf");
            else if (dep == "shaderc") mapped.push_back("shaderc");
            else if (dep == "vulkan-headers") mapped.push_back("vulkan-headers");
            else if (dep == "wayland-protocols") mapped.push_back("wayland-protocols");
            else if (dep == "wayland-client-dev") mapped.push_back("wayland");
            else if (dep == "xkbcommon-dev") mapped.push_back("libxkbcommon");
            else if (dep == "luajit-dev") mapped.push_back("luajit");
            else if (dep == "vulkan-icd-loader") mapped.push_back("vulkan-icd-loader");
            else if (dep == "wayland-client") mapped.push_back("wayland");
            else if (dep == "xkbcommon") mapped.push_back("libxkbcommon");
            else if (dep == "libffi") mapped.push_back("libffi");
            else if (dep == "luajit") mapped.push_back("luajit");
        }
        else if (type == PackageManagerType::DNF) {
            if (dep == "rust") { mapped.push_back("rust"); mapped.push_back("cargo"); }
            else if (dep == "c-compiler") { mapped.push_back("gcc"); mapped.push_back("gcc-c++"); }
            else if (dep == "cmake") mapped.push_back("cmake");
            else if (dep == "pkg-config") mapped.push_back("pkgconf");
            else if (dep == "shaderc") mapped.push_back("shaderc-devel");
            else if (dep == "vulkan-headers") mapped.push_back("vulkan-headers");
            else if (dep == "wayland-protocols") mapped.push_back("wayland-protocols-devel");
            else if (dep == "wayland-client-dev") mapped.push_back("wayland-devel");
            else if (dep == "xkbcommon-dev") mapped.push_back("libxkbcommon-devel");
            else if (dep == "luajit-dev") mapped.push_back("luajit-devel");
            else if (dep == "vulkan-icd-loader") mapped.push_back("vulkan-loader");
            else if (dep == "wayland-client") mapped.push_back("wayland");
            else if (dep == "xkbcommon") mapped.push_back("libxkbcommon");
            else if (dep == "libffi") mapped.push_back("libffi");
            else if (dep == "luajit") mapped.push_back("luajit");
        }
    }
    
    return mapped;
}

std::string PackageManager::generate_install_command(PackageManagerType type, const std::vector<std::string>& packages) {
    if (packages.empty()) return "";
    
    std::string cmd;
    if (type == PackageManagerType::APT) {
        cmd = "apt-get install -y";
    } else if (type == PackageManagerType::PACMAN) {
        cmd = "pacman -S --noconfirm --needed";
    } else if (type == PackageManagerType::DNF) {
        cmd = "dnf install -y";
    } else {
        return "";
    }
    
    for (const auto& pkg : packages) {
        cmd += " " + pkg;
    }
    
    return cmd;
}

bool PackageManager::install_dependencies(const std::vector<std::string>& dependencies, const std::string& sudo_password) {
    PackageManagerType type = detect_package_manager();
    if (type == PackageManagerType::UNKNOWN) {
        std::cerr << "Cannot install dependencies: Unknown package manager." << std::endl;
        return false;
    }
    
    std::vector<std::string> mapped_deps = map_dependencies(type, dependencies);
    if (mapped_deps.empty()) {
        std::cout << "No dependencies to install for this package manager." << std::endl;
        return true;
    }

    std::string cmd = generate_install_command(type, mapped_deps);
    std::cout << "Running: " << cmd << std::endl;
    
    int result = utils::SystemUtils::run_elevated(cmd, sudo_password);
    return (result == 0);
}

} // namespace core

cmake_minimum_required(VERSION 3.14)
project(ForgeInstaller VERSION 1.0.0 LANGUAGES CXX C)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED True)

set(CMAKE_POSITION_INDEPENDENT_CODE ON)

include(FetchContent)

message(STATUS "Fetching FLTK...")
# Using release-1.4.0 as it has modern CMake support (fltk::fltk target)
FetchContent_Declare(
    fltk
    GIT_REPOSITORY https://github.com/fltk/fltk.git
    GIT_TAG        release-1.4.5
)
set(OPTION_BUILD_SHARED_LIBS OFF CACHE BOOL "Disable FLTK shared libs" FORCE)
set(FLTK_BUILD_EXAMPLES OFF CACHE BOOL "Disable FLTK examples" FORCE)
set(FLTK_BUILD_TEST OFF CACHE BOOL "Disable FLTK tests" FORCE)
set(FLTK_BUILD_HTML_DOCS OFF CACHE BOOL "Disable FLTK docs" FORCE)
FetchContent_MakeAvailable(fltk)

message(STATUS "Fetching nlohmann_json...")
FetchContent_Declare(
    json
    GIT_REPOSITORY https://github.com/nlohmann/json.git
    GIT_TAG        v3.11.3
)
FetchContent_MakeAvailable(json)

message(STATUS "Fetching Catch2...")
FetchContent_Declare(
    Catch2
    GIT_REPOSITORY https://github.com/catchorg/Catch2.git
    GIT_TAG        v3.5.2
)
FetchContent_MakeAvailable(Catch2)

find_package(CURL REQUIRED)

add_executable(forge-installer
    src/main.cpp
    src/core/package_manager.cpp
    src/core/network_client.cpp
    src/core/extractor.cpp
    src/core/installer_state.cpp
    src/utils/system_utils.cpp
    src/gui/main_window.cpp
    src/gui/step_welcome.cpp
    src/gui/step_mode_selection.cpp
    src/gui/step_progress.cpp
    src/gui/step_uninstall_confirm.cpp
    src/gui/step_uninstall_progress.cpp
    src/core/uninstaller.cpp
)

target_include_directories(forge-installer PRIVATE 
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)

# FLTK 1.4 provides the fltk::fltk ALIAS target
target_link_libraries(forge-installer PRIVATE
    fltk::fltk
    CURL::libcurl
    nlohmann_json::nlohmann_json
)

enable_testing()
add_executable(forge-installer-tests
    tests/test_main.cpp
    src/core/package_manager.cpp
    src/core/network_client.cpp
    src/core/extractor.cpp
    src/core/installer_state.cpp
    src/utils/system_utils.cpp
)
target_link_libraries(forge-installer-tests PRIVATE
    Catch2::Catch2WithMain
    nlohmann_json::nlohmann_json
    CURL::libcurl
)
target_include_directories(forge-installer-tests PRIVATE 
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)

#pragma once
#include <string>

namespace core {

struct ReleaseInfo {
    std::string version_tag;
    std::string binary_url;
    std::string source_url;
};

class NetworkClient {
public:
    // Fetches the latest release info from GitHub API
    static ReleaseInfo get_latest_release();
    
    // Downloads a file from the given url to the specified destination path
    // Returns true on success, false otherwise
    static bool download_file(const std::string& url, const std::string& dest_path);
};

} // namespace core

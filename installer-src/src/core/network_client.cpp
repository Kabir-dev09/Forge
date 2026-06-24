#include "network_client.h"
#include <curl/curl.h>
#include <nlohmann/json.hpp>
#include <fstream>
#include <stdexcept>

namespace core {

static size_t WriteCallback(void* contents, size_t size, size_t nmemb, void* userp) {
    size_t total_size = size * nmemb;
    ((std::string*)userp)->append((char*)contents, total_size);
    return total_size;
}

static size_t FileWriteCallback(void* ptr, size_t size, size_t nmemb, void* stream) {
    size_t total_size = size * nmemb;
    std::ofstream* out = static_cast<std::ofstream*>(stream);
    out->write(static_cast<char*>(ptr), total_size);
    return total_size;
}

ReleaseInfo NetworkClient::get_latest_release() {
    CURL* curl = curl_easy_init();
    if (!curl) {
        throw std::runtime_error("Failed to initialize cURL");
    }

    std::string readBuffer;
    const char* api_url = "https://api.github.com/repos/Kabir-dev09/Forge/releases/latest";
    
    curl_easy_setopt(curl, CURLOPT_URL, api_url);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, WriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &readBuffer);
    curl_easy_setopt(curl, CURLOPT_USERAGENT, "Forge-Installer/1.0");
    curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, 1L);

    CURLcode res = curl_easy_perform(curl);
    
    long http_code = 0;
    curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &http_code);
    
    curl_easy_cleanup(curl);

    if (res != CURLE_OK) {
        throw std::runtime_error(std::string("cURL request failed: ") + curl_easy_strerror(res));
    }
    
    if (http_code != 200) {
        throw std::runtime_error("GitHub API request failed with HTTP " + std::to_string(http_code));
    }

    auto json = nlohmann::json::parse(readBuffer);
    if (!json.contains("tag_name")) {
        throw std::runtime_error("Invalid JSON response from GitHub API: missing tag_name");
    }

    std::string tag_name = json["tag_name"];
    
    ReleaseInfo info;
    info.version_tag = tag_name;
    info.binary_url = "https://github.com/Kabir-dev09/Forge/releases/download/" + tag_name + "/forge-binary-" + tag_name + ".tar.gz";
    info.source_url = "https://github.com/Kabir-dev09/Forge/releases/download/" + tag_name + "/forge-source-" + tag_name + ".tar.gz";
    
    return info;
}

bool NetworkClient::download_file(const std::string& url, const std::string& dest_path) {
    CURL* curl = curl_easy_init();
    if (!curl) return false;

    std::ofstream out(dest_path, std::ios::binary);
    if (!out.is_open()) {
        curl_easy_cleanup(curl);
        return false;
    }

    curl_easy_setopt(curl, CURLOPT_URL, url.c_str());
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, FileWriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &out);
    curl_easy_setopt(curl, CURLOPT_USERAGENT, "Forge-Installer/1.0");
    curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, 1L);

    CURLcode res = curl_easy_perform(curl);
    
    long http_code = 0;
    curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &http_code);
    
    curl_easy_cleanup(curl);
    out.close();

    if (res != CURLE_OK || http_code >= 400) {
        std::remove(dest_path.c_str());
        return false;
    }

    return true;
}

} // namespace core

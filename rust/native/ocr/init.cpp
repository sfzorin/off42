// Unicode-safe model loading. No current-directory/environment search or downloads.
#include <tesseract/baseapi.h>
#include <filesystem>
#include <fstream>
#include <vector>
static bool read_model(const char* name, std::vector<char>* out) {
  try {
    std::ifstream file(std::filesystem::u8path(name), std::ios::binary | std::ios::ate);
    if (!file) return false;
    auto size = file.tellg();
    if (size <= 0 || size > 64 * 1024 * 1024) return false;
    out->resize(static_cast<size_t>(size));
    file.seekg(0);
    return bool(file.read(out->data(), size));
  } catch (...) { return false; }
}
extern "C" int off42_ocr_init(tesseract::TessBaseAPI* api, const char* data_path,
                              const char* languages) noexcept {
  try {
    return api->Init(data_path, 0, languages, tesseract::OEM_LSTM_ONLY,
                     nullptr, 0, nullptr, nullptr, false, read_model);
  } catch (...) { return -1; }
}

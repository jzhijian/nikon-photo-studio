// Local adapter for the separately licensed Nikon Image SDK. No Nikon source
// or binary is embedded. Camera originals are opened for reading only.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <ole2.h>
#include <stdio.h>
#include <stdlib.h>
#include <wchar.h>
#include <string.h>
#include <stdint.h>
#include "Nkfl_Interface.h"

static_assert(sizeof(void*) == 8 && sizeof(unsigned long) == 4, "Windows x64 is required");
static bool regularFile(const wchar_t* path) {
    DWORD attributes = GetFileAttributesW(path);
    return attributes != INVALID_FILE_ATTRIBUTES && !(attributes & FILE_ATTRIBUTE_DIRECTORY);
}
static bool absolute(const wchar_t* source, wchar_t* destination) {
    DWORD size = GetFullPathNameW(source, 32768, destination, nullptr);
    return size && size < 32000;
}
static bool utf8(const wchar_t* source, char* destination, int capacity) {
    return WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, source, -1, destination, capacity, nullptr, nullptr) != 0;
}
static void printString(const char* value) {
    putchar('"');
    for (const unsigned char* p = reinterpret_cast<const unsigned char*>(value); *p; ++p) {
        if (*p == '"' || *p == '\\') printf("\\%c", *p);
        else if (*p < 32) printf("\\u%04x", *p);
        else putchar(*p);
    }
    putchar('"');
}

int wmain(int argc, wchar_t** argv) {
    HMODULE module = nullptr;
    Nkfl_EntryProcPtr entry = nullptr;
    DLL_DIRECTORY_COOKIE dllDirectory = nullptr;
    bool sdkLoaded = false, initialized = false, sessionOpened = false, oleInitialized = false, rendered = false;
    bool savedEdits = false, renderRequested = false, editStateRead = false;
    unsigned long result = 0, closeResult = 0;
    NkflLibraryParam library{};
    NkflSessionParam session{};
    NkflImageInfoParam image{};
    NkflFileInfoParam file{};
    NkflDevelopColorMode mode{};
    NkflRawDevelopment_RawParameterSet parameterSet{};
    NkflEditState editState{};
    NkflRawDevelopment_RawQuality quality{};
    NkflRawDevelopmentParam development{};
    NkflOutputProfileParam colorProfile{};
    NkflOutputDeviceProfile deviceProfile{};
    NkflImageParam imageData{};
    void* pixels = nullptr;
    uint64_t pixelBytes = 0;
    wchar_t sdk[32768] = {}, dll[32768] = {}, input[32768] = {}, profiles[32768] = {};
    wchar_t tempRoot[MAX_PATH] = {}, tempFile[MAX_PATH] = {};
    char inputUtf8[32768 * 3] = {};
    wchar_t outputPath[32768] = {};
    wchar_t executableDirectory[32768] = {};
    char outputUtf8[32768 * 3] = {};
    char error[512] = {};
    int exitCode = 0;

#define FAIL(...) do { snprintf(error, sizeof(error), __VA_ARGS__); exitCode = 1; goto cleanup; } while (0)
#define CHECK(call, stage) do { result = (call); if (result != kNkfl_Code_None) FAIL("%s returned 0x%08lx", stage, result); } while (0)
    if (argc == 5 && !wcscmp(argv[4], L"--saved-edits")) savedEdits = true;
    if (argc < 2 || argc > 5 || (argc == 5 && !savedEdits))
        FAIL("Usage: nikon-image-probe.exe <SDK Release Bin directory> [NEF path] [new output.ppm [--saved-edits]]");
    for (int i = 1; i < argc - (savedEdits ? 1 : 0); ++i) {
        if (!wcscmp(argv[i], L"--saved-edits"))
            FAIL("--saved-edits requires SDK directory, NEF input, and a new PPM output, followed by --saved-edits");
    }
    renderRequested = argc >= 4;
    // The official MFC sample initializes OLE before opening the Image SDK.
    if (FAILED(OleInitialize(nullptr))) FAIL("OLE initialization failed");
    oleInitialized = true;
    if (!absolute(argv[1], sdk)) FAIL("Invalid SDK directory");
    swprintf(dll, 32768, L"%ls\\NkImgSDK.dll", sdk);
    if (!regularFile(dll)) FAIL("Selected directory does not contain NkImgSDK.dll");
    if (argc >= 3) {
        if (!absolute(argv[2], input) || !regularFile(input)) FAIL("Input image does not exist");
        if (!utf8(input, inputUtf8, sizeof(inputUtf8))) FAIL("Input path cannot be represented as UTF-8");
    }
    if (renderRequested) {
        if (!absolute(argv[3], outputPath) || !utf8(outputPath, outputUtf8, sizeof(outputUtf8))) FAIL("Invalid output path");
        if (GetFileAttributesW(outputPath) != INVALID_FILE_ATTRIBUTES) FAIL("Output path already exists; choose a new file");
        const wchar_t* suffix = wcsrchr(outputPath, L'.');
        if (!suffix || _wcsicmp(suffix, L".ppm")) FAIL("Output must use the .ppm extension");
        // Nikon's raw engine resolves supporting data relative to the EXE,
        // not merely the process CWD or the Windows DLL search directories.
        DWORD executableLength = GetModuleFileNameW(nullptr, executableDirectory, 32768);
        if (!executableLength || executableLength >= 32768) FAIL("Cannot resolve helper executable location");
        wchar_t* separator = wcsrchr(executableDirectory, L'\\');
        if (!separator) FAIL("Cannot resolve helper executable directory");
        *separator = 0;
        size_t sdkLength = wcslen(sdk);
        while (sdkLength > 3 && (sdk[sdkLength - 1] == L'\\' || sdk[sdkLength - 1] == L'/')) sdk[--sdkLength] = 0;
        if (_wcsicmp(executableDirectory, sdk)) FAIL("Rendering requires nikon-image-probe.exe beside NkImgSDK.dll and the SDK supporting files; copy only this helper into the selected SDK Release directory");
    }
    if (!GetTempPathW(MAX_PATH, tempRoot) || !GetTempFileNameW(tempRoot, L"nps", 0, tempFile)) FAIL("Cannot allocate SDK scratch file");
    library.ulSize = sizeof(library);
    library.ulVersion = 0x01000000;
    library.ulVMMemorySize = 512;
    swprintf(dll, 32768, L"%ls\\..\\..\\..\\Profiles\\", sdk);
    if (!absolute(dll, profiles)) FAIL("Invalid SDK color profile path");
    swprintf(dll, 32768, L"%ls\\NkImgSDK.dll", sdk);
    if (!utf8(tempFile, reinterpret_cast<char*>(library.VMFileInfo), sizeof(library.VMFileInfo))
        || !utf8(profiles, reinterpret_cast<char*>(library.DefProfPath), sizeof(library.DefProfPath)))
        FAIL("SDK scratch path is too long");
    if (!SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_USER_DIRS)) FAIL("Cannot configure DLL search paths");
    dllDirectory = AddDllDirectory(sdk);
    if (!dllDirectory) FAIL("Cannot register SDK DLL directory");
    // The official sample is launched beside its data files (Elm.nlf/prm.bin).
    // This changes only the isolated helper process working directory.
    if (!SetCurrentDirectoryW(sdk)) FAIL("Cannot select SDK working directory");
    module = LoadLibraryExW(dll, nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!module) FAIL("LoadLibrary NkImgSDK.dll failed: Windows error %lu", GetLastError());
    sdkLoaded = true;
    entry = reinterpret_cast<Nkfl_EntryProcPtr>(GetProcAddress(module, "Nkfl_Entry"));
    if (!entry) FAIL("Nkfl_Entry export missing");
    CHECK(entry(kNkfl_Cmd_OpenLibrary, &library), "OpenLibrary"); initialized = true;
    mode.ulSize = sizeof(mode);
    mode.lDevelopColorMode = kNkfl_DevelopColorMode_AppliedInCamera;
    CHECK(entry(kNkfl_Cmd_SetDevelopColorMode, &mode), "SetDevelopColorMode");
    if (argc >= 3) {
        session.ulSize = sizeof(session);
        session.ulType = kNkfl_Source_FileName_UTF8;
        session.pFileInfo = inputUtf8;
        result = entry(kNkfl_Cmd_OpenSession, &session);
        // Nikon's sample treats warning responses as open sessions. Close those
        // too, but report the warning instead of claiming successful decoding.
        sessionOpened = result == kNkfl_Code_None || (result & 0x0f00) != 0;
        if (result != kNkfl_Code_None) FAIL("OpenSession returned 0x%08lx", result);
        file.ulSize = sizeof(file); file.ulSessionID = session.ulSessionID;
        CHECK(entry(kNkfl_Cmd_GetFileInfo, &file), "GetFileInfo");
        image.ulSize = sizeof(image); image.ulSessionID = session.ulSessionID;
        CHECK(entry(kNkfl_Cmd_GetImageInfo, &image), "GetImageInfo");
        if (renderRequested) {
            development.ulSize = sizeof(development); development.ulSessionID = session.ulSessionID;
            parameterSet.ulSize = sizeof(parameterSet);
            if (savedEdits) {
                editState.ulSize = sizeof(editState); editState.ulSessionID = session.ulSessionID;
                CHECK(entry(kNkfl_Cmd_GetEditState, &editState), "GetEditState");
                editStateRead = true;
                if (editState.ulEditState != kNkfl_EditState_ApplicableEdit)
                    FAIL("Saved edits are not applicable (editState=%lu); save edits embedded in a NEF/NRW copy using NX Studio. Sidecar files are not supported; refusing as-shot fallback", editState.ulEditState);
            } else {
                parameterSet.ulParamterSet = kNkfl_RawParameterSet_AsShot;
                development.ulRawDevelopment = kNkfl_RawDevelopment_RawParameterSet; development.pData = &parameterSet;
                CHECK(entry(kNkfl_Cmd_RawDevelopment, &development), "SetAsShotDevelopment");
            }
            quality.ulSize = sizeof(quality); quality.ulQuality = kNkfl_RawQuality_High;
            development.ulRawDevelopment = kNkfl_RawDevelopment_RawQuality; development.pData = &quality;
            CHECK(entry(kNkfl_Cmd_RawDevelopment, &development), "SetHighQualityDevelopment");
            if (savedEdits) {
                // LastSaveSetting locks subsequent RawDevelopment adjustments.
                // Every warning, including missing/inapplicable edits, is a failure.
                parameterSet.ulParamterSet = kNkfl_RawParameterSet_LastSaveSetting;
                development.ulRawDevelopment = kNkfl_RawDevelopment_RawParameterSet; development.pData = &parameterSet;
                CHECK(entry(kNkfl_Cmd_RawDevelopment, &development), "SetSavedEditsDevelopment (as-shot fallback is not allowed)");
                CHECK(entry(kNkfl_Cmd_GetImageInfo, &image), "GetSavedEditImageInfo");
            }
            image.ulByteDepth = 1; image.ulColor = kNkfl_Color_RGB;
            // Keep the SDK's AsShot/LastSaveSetting orientation without an extra transform.
            image.ulOrientation = kNkfl_Orientation_CW0;
            CHECK(entry(kNkfl_Cmd_SetImageInfo, &image), "SetImageInfo");
            colorProfile.ulSize = sizeof(colorProfile); colorProfile.ulSessionID = session.ulSessionID;
            swprintf(dll, 32768, L"%ls\\NKsRGB.icm", profiles);
            if (!regularFile(dll) || !utf8(dll, reinterpret_cast<char*>(colorProfile.OutputProfile), sizeof(colorProfile.OutputProfile))) FAIL("Official sRGB output profile unavailable");
            CHECK(entry(kNkfl_Cmd_SetOutputProfile_UTF8, &colorProfile), "SetOutputProfile");
            // SDK 1.47 separates the working/output profile from the output
            // device profile. Do not inherit a possibly incompatible monitor ICC.
            deviceProfile.ulSize = sizeof(deviceProfile); deviceProfile.ulSessionID = session.ulSessionID;
            memcpy(deviceProfile.OutputDeviceProfile, colorProfile.OutputProfile, sizeof(deviceProfile.OutputDeviceProfile));
            CHECK(entry(kNkfl_Cmd_SetOutputDeviceProfile, &deviceProfile), "SetOutputDeviceProfile");
            CHECK(entry(kNkfl_Cmd_GetImageInfo, &image), "GetRenderImageInfo");
            pixelBytes = static_cast<uint64_t>(image.ulWidth) * image.ulHeight * 3;
            if (!image.ulWidth || !image.ulHeight || image.ulByteDepth != 1 || image.ulColor != kNkfl_Color_RGB || pixelBytes > 512 * 1024 * 1024) FAIL("Unsupported or oversized RGB render dimensions");
            pixels = malloc(static_cast<size_t>(pixelBytes));
            if (!pixels) FAIL("Unable to allocate render buffer");
            imageData.ulSize = sizeof(imageData); imageData.ulSessionID = session.ulSessionID;
            imageData.rectArea.right = image.ulWidth; imageData.rectArea.bottom = image.ulHeight;
            imageData.ulDataSize = static_cast<unsigned long>(pixelBytes); imageData.pData = pixels;
            CHECK(entry(kNkfl_Cmd_GetImageData, &imageData), "GetImageData");
            if (imageData.ulDataSize != pixelBytes) FAIL("SDK returned an unexpected RGB buffer size");
            rendered = true;
        }
    }
cleanup:
    if (sessionOpened && entry) {
        closeResult = entry(kNkfl_Cmd_CloseSession, &session);
        if (closeResult && !exitCode) { snprintf(error, sizeof(error), "CloseSession returned 0x%08lx", closeResult); exitCode = 1; }
    }
    if (initialized && entry) {
        closeResult = entry(kNkfl_Cmd_CloseLibrary, nullptr);
        if (closeResult && !exitCode) { snprintf(error, sizeof(error), "CloseLibrary returned 0x%08lx", closeResult); exitCode = 1; }
    }
    if (!closeResult && module) FreeLibrary(module);
    if (dllDirectory && !closeResult) RemoveDllDirectory(dllDirectory);
    if (tempFile[0] && !closeResult) DeleteFileW(tempFile);
    if (oleInitialized && !closeResult) OleUninitialize();
    if (!exitCode && rendered) {
        HANDLE outputFile = CreateFileW(outputPath, GENERIC_WRITE, 0, nullptr, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
        if (outputFile == INVALID_HANDLE_VALUE) { snprintf(error, sizeof(error), "Cannot create output without overwriting; Windows error %lu", GetLastError()); exitCode = 1; }
        else {
            char header[100]; DWORD written = 0;
            int headerSize = snprintf(header, sizeof(header), "P6\n%lu %lu\n255\n", image.ulWidth, image.ulHeight);
            bool saved = WriteFile(outputFile, header, headerSize, &written, nullptr) && written == static_cast<DWORD>(headerSize);
            if (saved) saved = WriteFile(outputFile, pixels, static_cast<DWORD>(pixelBytes), &written, nullptr) && written == pixelBytes;
            if (saved) saved = FlushFileBuffers(outputFile) != 0;
            CloseHandle(outputFile);
            if (!saved) { DeleteFileW(outputPath); snprintf(error, sizeof(error), "RGB output write failed"); exitCode = 1; }
        }
    }
    free(pixels);
    printf("{\"schemaVersion\":1,\"sdkLoaded\":%s,\"initialized\":%s,\"inputOpened\":%s",
        sdkLoaded ? "true" : "false", initialized ? "true" : "false", sessionOpened ? "true" : "false");
    if (exitCode) { printf(",\"error\":"); printString(error); }
    else if (sessionOpened) printf(",\"format\":%lu,\"width\":%lu,\"height\":%lu,\"byteDepth\":%lu,\"color\":%lu,\"orientation\":%lu",
        file.ulFormat, image.ulWidth, image.ulHeight, image.ulByteDepth, image.ulColor, image.ulOrientation);
    printf(",\"rendered\":%s", rendered && !exitCode ? "true" : "false");
    if (renderRequested) printf(",\"requestedSettings\":\"%s\"", savedEdits ? "saved-edits" : "as-shot");
    if (editStateRead) printf(",\"editState\":%lu", editState.ulEditState);
    if (rendered && !exitCode) { printf(",\"outputPath\":"); printString(outputUtf8); printf(",\"colorSpace\":\"sRGB\",\"settings\":\"%s\"", savedEdits ? "saved-edits" : "as-shot"); }
    puts("}");
    return exitCode;
}

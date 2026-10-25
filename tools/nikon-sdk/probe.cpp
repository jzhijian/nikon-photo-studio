// Application-owned read-only probe. Nikon headers/binaries are supplied
// separately under Nikon's SDK agreement and are not redistributed here.
// Win32/C facilities keep the SDK ABI isolated from GNU C++ and Rust runtimes.
#include <stdio.h>
#include <stdlib.h>
#include <stdarg.h>
#include <string.h>
#include <wchar.h>
#include <stdint.h>
#include "Maid3d1.h"
#include "Maid3.h"

static_assert(sizeof(void*) == 8 && sizeof(ULONG) == 4 && sizeof(NKPARAM) == 8,
    "This helper requires the 64-bit Windows SDK ABI");
// Maid3.h explicitly uses #pragma pack(push, 2).
static_assert(sizeof(NkMAIDDeviceInfo) == 138 && sizeof(NkMAIDEnumDevices) == 16
    && sizeof(NkMAIDCapInfo) == 272 && sizeof(NkMAIDCSCallback) == 48,
    "SDK structures have unexpected packing");

struct Allocation { void* value; Allocation* next; };
static SRWLOCK allocationLock = SRWLOCK_INIT;
static Allocation* allocations = nullptr;
static void* CALLBACK allocate(size_t size) {
    void* value = calloc(1, size);
    Allocation* item = value ? static_cast<Allocation*>(malloc(sizeof(Allocation))) : nullptr;
    if (!item) { free(value); return nullptr; }
    item->value = value;
    AcquireSRWLockExclusive(&allocationLock);
    item->next = allocations; allocations = item;
    ReleaseSRWLockExclusive(&allocationLock);
    return value;
}
static void CALLBACK release(void* value) {
    if (!value) return;
    AcquireSRWLockExclusive(&allocationLock);
    Allocation** next = &allocations;
    while (*next && (*next)->value != value) next = &(*next)->next;
    if (*next) { Allocation* item = *next; *next = item->next; free(item->value); free(item); }
    ReleaseSRWLockExclusive(&allocationLock);
}
static void releaseRemaining() {
    // Called only after FreeSDK has stopped all callbacks.
    while (allocations) release(allocations->value);
}
static void CALLBACK event(NKREF, ULONG, NKPARAM) {}
static void CALLBACK progress(ULONG, ULONG, NKREF, ULONG, ULONG) {}
static ULONG CALLBACK prompt(NKREF, LPNkMAIDUIRequestInfo) { return kNkMAIDUIRequestResult_Cancel; }
static NKERROR CALLBACK data(NKREF, LPVOID, LPVOID) { return kNkMAIDResult_NoError; }
static NKERROR CALLBACK live(NKREF, LPNkMAIDLiveViewData) { return kNkMAIDResult_NoError; }

struct Json {
    char bytes[256 * 1024];
    size_t length;
    bool failed;
    void append(const char* format, ...) {
        if (failed) return;
        va_list args; va_start(args, format);
        int count = vsnprintf(bytes + length, sizeof(bytes) - length, format, args);
        va_end(args);
        if (count < 0 || static_cast<size_t>(count) >= sizeof(bytes) - length) failed = true;
        else length += static_cast<size_t>(count);
    }
    void string(const char* value, size_t maxLength) {
        append("\"");
        for (size_t i = 0; i < maxLength && value[i]; ++i) {
            const unsigned char c = static_cast<unsigned char>(value[i]);
            if (c == '"' || c == '\\') append("\\%c", c);
            else if (c < 32 || c >= 127) append("\\u%04x", c);
            else append("%c", c);
        }
        append("\"");
    }
};
static bool regularFile(const wchar_t* path) {
    DWORD attributes = GetFileAttributesW(path);
    return attributes != INVALID_FILE_ATTRIBUTES && !(attributes & FILE_ATTRIBUTE_DIRECTORY);
}

int wmain(int argc, wchar_t** argv) {
    HMODULE module = nullptr;
    LPMAIDInitializeSDKProc initialize = nullptr;
    LPMAIDFreeSDKProc shutdown = nullptr;
    LPMAIDConnectDeviceProc connect = nullptr;
    LPMAIDDisconnectDeviceProc disconnect = nullptr;
    bool initializationAttempted = false, initialized = false, connected = false, sdkLoaded = false;
    DLL_DIRECTORY_COOKIE dllDirectory = nullptr;
    NkMAIDCSCallback callbacks{}; // Valid until FreeSDK returns.
    LPNkMAIDEnumDevices devices = nullptr;
    LPNkMAIDEnumCapInfo capabilities = nullptr;
    Json output{};
    char error[512] = {};
    wchar_t directory[32768] = {}, path[32768] = {};
    const wchar_t* local = _wgetenv(L"LOCALAPPDATA");
    const wchar_t* profiles[] = {L"DC_PTP_Config.config", L"MaidLayer.config", L"RangeValue.config"};
    long result = 0;
    DWORD pathSize = 0;
    int exitCode = 0;

#define FAIL(...) do { snprintf(error, sizeof(error), __VA_ARGS__); exitCode = 1; goto cleanup; } while (0)
#define LOAD(member, type, name) do { member = reinterpret_cast<type>(GetProcAddress(module, name)); if (!member) FAIL("Missing SDK export: %s", name); } while (0)
#define CHECK(call, name) do { result = (call); if (result != 0) FAIL("%s returned %ld", name, result); } while (0)

    if (argc != 2) FAIL("Usage: nikon-sdk-probe.exe <SDK Module/Win/BinaryFile directory>");
    pathSize = GetFullPathNameW(argv[1], 32768, directory, nullptr);
    if (!pathSize || pathSize > 32000) FAIL("Invalid SDK directory path");
    swprintf(path, 32768, L"%ls\\ControlServiceLayer.dll", directory);
    if (!regularFile(path)) FAIL("Selected directory does not contain ControlServiceLayer.dll");
    if (!local || wcslen(local) > 32000) FAIL("LOCALAPPDATA unavailable");
    for (const wchar_t* name : profiles) {
        swprintf(path, 32768, L"%ls\\Nikon\\NXTether\\%ls", local, name);
        if (!regularFile(path)) FAIL("SDK profiles missing; run the profile installer before probing");
    }
    if (!SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_USER_DIRS))
        FAIL("Cannot configure SDK DLL search paths");
    dllDirectory = AddDllDirectory(directory);
    if (!dllDirectory) FAIL("Cannot add SDK DLL directory");
    swprintf(path, 32768, L"%ls\\ControlServiceLayer.dll", directory);
    module = LoadLibraryExW(path, nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!module) FAIL("Cannot load SDK DLL; Windows error %lu", GetLastError());
    sdkLoaded = true;
    LOAD(initialize, LPMAIDInitializeSDKProc, "InitializeSDK");
    LOAD(shutdown, LPMAIDFreeSDKProc, "FreeSDK");
    LOAD(connect, LPMAIDConnectDeviceProc, "ConnectDevice");
    LOAD(disconnect, LPMAIDDisconnectDeviceProc, "DisconnectDevice");
    callbacks.pfnEventProc = reinterpret_cast<LPNKFUNC>(event);
    callbacks.pUIReqProc = reinterpret_cast<LPNKFUNC>(prompt);
    callbacks.pProgressProc = reinterpret_cast<LPNKFUNC>(progress);
    callbacks.pDataProc = reinterpret_cast<LPNKFUNC>(data);
    callbacks.pLiveViewDataProc = reinterpret_cast<LPNKFUNC>(live);
    initializationAttempted = true;
    CHECK(initialize(allocate, release, &callbacks, &devices, nullptr), "InitializeSDK");
    initialized = true;
    output.append("{\"schemaVersion\":1,\"sdkLoaded\":true,\"canWritePreset\":false,\"devices\":[");
    if (devices && (devices->ulElements > 100 || (devices->ulElements && !devices->pDeviceData))) FAIL("Invalid SDK device list");
    for (ULONG i = 0; devices && i < devices->ulElements; ++i) {
        const NkMAIDDeviceInfo& device = devices->pDeviceData[i];
        if (i) output.append(",");
        output.append("{\"name\":"); output.string(device.Name, sizeof(device.Name));
        output.append(",\"firmware\":"); output.string(device.Version, sizeof(device.Version));
        output.append(",\"available\":%s,\"capabilities\":[", device.Availability ? "true" : "false");
        if (device.Availability) {
            CHECK(connect(device.ID, &capabilities), "ConnectDevice"); connected = true;
            if (capabilities && (capabilities->ulCapCount > 10000 || (capabilities->ulCapCount && !capabilities->pCapArray))) FAIL("Invalid SDK capability list");
            bool first = true;
            for (ULONG j = 0; capabilities && j < capabilities->ulCapCount; ++j) {
                const NkMAIDCapInfo& cap = capabilities->pCapArray[j];
                if (cap.ulID != kNkMAIDCapability_PictureControl && cap.ulID != kNkMAIDCapability_PictureControlData
                    && cap.ulID != kNkMAIDCapability_PictureControlDataEx && cap.ulID != kNkMAIDCapability_PictureControlDataEx2) continue;
                if (!first) output.append(","); first = false;
                output.append("{\"id\":%lu,\"canGet\":%s,\"canSet\":%s}", cap.ulID,
                    (cap.ulOperations & kNkMAIDCapOperation_Get) ? "true" : "false",
                    (cap.ulOperations & kNkMAIDCapOperation_Set) ? "true" : "false");
            }
            CHECK(disconnect(), "DisconnectDevice"); connected = false;
            if (capabilities) { release(capabilities->pCapArray); release(capabilities); capabilities = nullptr; }
        }
        output.append("]}");
    }
    output.append("],\"note\":\"Capability flags do not verify preset compatibility or successful writes\"}");
    if (output.failed) FAIL("SDK report exceeds output limit");

cleanup:
    if (connected && disconnect) disconnect();
    result = initialized && shutdown ? shutdown() : 0;
    if (!result && (!initializationAttempted || initialized)) {
        releaseRemaining();
        if (module) FreeLibrary(module);
        if (dllDirectory) RemoveDllDirectory(dllDirectory);
    } else if (result) {
        snprintf(error, sizeof(error), "FreeSDK returned %ld", result); exitCode = 1;
    }
    // Failed initialization/shutdown leaves DLL/code loaded until helper process
    // exit, so no asynchronous callback runs into unmapped code.
    if (exitCode) {
        output.length = 0; output.failed = false;
        output.append("{\"schemaVersion\":1,\"sdkLoaded\":%s,\"canWritePreset\":false,\"error\":", sdkLoaded ? "true" : "false");
        output.string(error, sizeof(error)); output.append("}");
    }
    puts(output.bytes);
    return exitCode;
}

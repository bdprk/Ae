/* Runs a program attached to a real console whose output code page is 936 (a
   Chinese Windows default), then reads the screen buffer back and saves what the
   console displays as UTF-8. This is the "before/after" measurement for ADR-55:
   the bytes a program writes are not what a user sees. */
#include <stdio.h>
#include <string.h>
#include <windows.h>

int wmain(int argc, wchar_t **argv) {
    if (argc < 3) { fprintf(stderr, "usage: console_run <log> <program> [arg]\n"); return 2; }
    FreeConsole();
    if (!AllocConsole()) { fprintf(stderr, "no console\n"); return 1; }
    /* Third argument picks the code page, so the same program can be observed
       under a Chinese code page (936) and under UTF-8 (65001). */
    const unsigned page = argc > 4 ? (unsigned)_wtoi(argv[4]) : 936;
    SetConsoleOutputCP(page);
    SetConsoleCP(page);

    SECURITY_ATTRIBUTES inherit = { sizeof inherit, NULL, TRUE };
    HANDLE con = CreateFileW(L"CONOUT$", GENERIC_READ | GENERIC_WRITE,
                             FILE_SHARE_READ | FILE_SHARE_WRITE, &inherit,
                             OPEN_EXISTING, 0, NULL);
    if (con == INVALID_HANDLE_VALUE) { fprintf(stderr, "no conout\n"); return 1; }
    /* A default console buffer keeps ~30 rows; the long-text case needs the whole
       output to survive so the readback can be compared with what was written. */
    COORD size = { 120, 400 };
    SetConsoleScreenBufferSize(con, size);

    STARTUPINFOW si;
    PROCESS_INFORMATION pi;
    memset(&si, 0, sizeof si);
    si.cb = sizeof si;
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    si.hStdOutput = con;
    si.hStdError = con;
    if (!CreateProcessW(argv[2], argc > 3 ? argv[3] : argv[2], NULL, NULL, TRUE, 0, NULL, NULL, &si, &pi)) {
        fprintf(stderr, "CreateProcess failed: %lu\n", GetLastError());
        return 1;
    }
    WaitForSingleObject(pi.hProcess, INFINITE);
    DWORD status = 0;
    GetExitCodeProcess(pi.hProcess, &status);
    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);

    CONSOLE_SCREEN_BUFFER_INFO info;
    GetConsoleScreenBufferInfo(con, &info);
    FILE *log = _wfopen(argv[1], L"wb");
    wchar_t row[512];
    for (SHORT y = 0; y <= info.dwCursorPosition.Y; y++) {
        DWORD got = 0;
        COORD at = {0, y};
        if (!ReadConsoleOutputCharacterW(con, row, (DWORD)info.dwSize.X, at, &got)) continue;
        while (got > 0 && (row[got - 1] == L' ' || row[got - 1] == 0)) got -= 1;
        char utf8[1536];
        const int n = WideCharToMultiByte(CP_UTF8, 0, row, (int)got, utf8, sizeof utf8, NULL, NULL);
        if (n > 0) fwrite(utf8, 1, (size_t)n, log);
        fwrite("\n", 1, 1, log);
    }
    fclose(log);
    fprintf(stderr, "child exit=%lu cp=%u\n", status, GetConsoleOutputCP());
    return 0;
}

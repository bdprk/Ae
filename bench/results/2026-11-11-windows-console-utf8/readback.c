/* What the console actually displays differs from the bytes a program writes:
   the console decodes them with its output code page. This probe opens a real
   console, sends the same UTF-8 payload as raw bytes and as wide characters,
   and reads the screen buffer back after each. */
#include <stdio.h>
#include <string.h>
#include <windows.h>

static HANDLE g_con;
static FILE *g_log;

static void record(const char *label) {
    CONSOLE_SCREEN_BUFFER_INFO info;
    GetConsoleScreenBufferInfo(g_con, &info);
    SHORT row = (SHORT)(info.dwCursorPosition.Y - 1);
    COORD start = {0, row};
    wchar_t line[256];
    DWORD got = 0;
    ReadConsoleOutputCharacterW(g_con, line, 255, start, &got);
    fprintf(g_log, "%-6s (cp %u): ", label, GetConsoleOutputCP());
    for (DWORD i = 0; i < got; i++) {
        if (line[i] == L' ' || line[i] == 0) continue;
        fprintf(g_log, "U+%04X ", (unsigned)line[i]);
    }
    fprintf(g_log, "\n");
    fflush(g_log);
}

int main(void) {
    FreeConsole();
    if (!AllocConsole()) { fprintf(stderr, "no console\n"); return 1; }
    g_con = CreateFileW(L"CONOUT$", GENERIC_READ | GENERIC_WRITE,
                        FILE_SHARE_READ | FILE_SHARE_WRITE, NULL,
                        OPEN_EXISTING, 0, NULL);
    g_log = fopen("readback.txt", "w");
    if (g_con == INVALID_HANDLE_VALUE || !g_log) { fprintf(stderr, "no conout\n"); return 1; }

    const char *payload = "\xe4\xb8\xad\xe6\x96\x87\xe6\xb5\x8b\xe8\xaf\x95";  /* 中文测试 */
    DWORD written = 0;
    WriteFile(g_con, payload, (DWORD)strlen(payload), &written, NULL);
    WriteFile(g_con, "\r\n", 2, &written, NULL);
    record("bytes");

    wchar_t wide[64];
    int n = MultiByteToWideChar(CP_UTF8, 0, payload, -1, wide, 64);
    WriteConsoleW(g_con, wide, (DWORD)(n - 1), &written, NULL);
    WriteConsoleW(g_con, L"\r\n", 2, &written, NULL);
    record("wide");

    fclose(g_log);
    return 0;
}

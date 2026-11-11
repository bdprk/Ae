/* Is a non-BMP character lost by WriteConsoleW itself, or by the console's font
   and code page? Send a surrogate pair directly and read the buffer back. */
#include <stdio.h>
#include <string.h>
#include <windows.h>

int main(void) {
    FreeConsole();
    AllocConsole();
    SetConsoleOutputCP(936);
    HANDLE con = CreateFileW(L"CONOUT$", GENERIC_READ | GENERIC_WRITE,
                             FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING, 0, NULL);
    FILE *log = fopen("emoji.txt", "w");
    /* U+1F600 as a surrogate pair, written as wide characters. */
    const wchar_t pair[] = { 0xD83D, 0xDE00, 0 };
    DWORD written = 0;
    WriteConsoleW(con, pair, 2, &written, NULL);
    WriteConsoleW(con, L"\r\n", 2, &written, NULL);

    CONSOLE_SCREEN_BUFFER_INFO info;
    GetConsoleScreenBufferInfo(con, &info);
    COORD at = {0, (SHORT)(info.dwCursorPosition.Y - 1)};
    wchar_t row[64];
    DWORD got = 0;
    ReadConsoleOutputCharacterW(con, row, 63, at, &got);
    fprintf(log, "units read back:");
    for (DWORD i = 0; i < got; i++) fprintf(log, " U+%04X", (unsigned)row[i]);
    /* Same pair, this time through the UTF-8 conversion the runtime uses. */
    CONSOLE_FONT_INFOEX font;
    font.cbSize = sizeof font;
    GetCurrentConsoleFontEx(con, FALSE, &font);
    fprintf(log, "\nfont: %ls (%u x %u)\n", font.FaceName, font.dwFontSize.X, font.dwFontSize.Y);
    fclose(log);
    return 0;
}

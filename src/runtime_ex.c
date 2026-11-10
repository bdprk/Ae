// Fallible built-ins and platform helpers that name prelude enum types.
// Included after the enum layouts, so ae_e0..ae_e3 exist here.
// Invariant (checked by the emitter): the prelude registers exactly
//   0 ReadResult{Ok(string), Err(string)}
//   1 WriteResult{Ok(i64), Err(string)}
//   2 IntResult{Ok(i64), Err(string)}
//   3 DirResult{Ok(vec[string]), Err(string)}
// with Ok at variant index 0 and Err at index 1.

#include <errno.h>
#ifndef _WIN32
#include <dirent.h>
#include <sys/stat.h>
#endif

#define AE_READ_RESULT ae_e0
#define AE_READ_OK AE_E0_0
#define AE_READ_ERR AE_E0_1
#define AE_WRITE_RESULT ae_e1
#define AE_WRITE_OK AE_E1_0
#define AE_WRITE_ERR AE_E1_1
#define AE_INT_RESULT ae_e2
#define AE_INT_OK AE_E2_0
#define AE_INT_ERR AE_E2_1
#define AE_DIR_RESULT ae_e3
#define AE_DIR_OK AE_E3_0
#define AE_DIR_ERR AE_E3_1

static ae_string ae_io_message(int code, size_t line, size_t column) {
    const char *text = strerror(code == 0 ? EIO : code);
    return ae_str_from(text ? text : "I/O error", line, column);
}
// Error text carrying a platform status code where strerror does not apply.
static ae_string ae_status_message(const char *what, unsigned long code, size_t line, size_t column) {
    char buffer[160];
    const int length = snprintf(buffer, sizeof buffer, "%s (status %lu)", what, code);
    if (length < 0) return ae_str_from(what, line, column);
    return ae_str_from_len(buffer, (int64_t)(length < (int)sizeof buffer ? length : (int)sizeof buffer - 1), line, column);
}

static AE_READ_RESULT ae_try_read_file(const char *path, size_t line, size_t column) {
    errno = 0;
    FILE *f = ae_fopen(path, "rb");
    if (!f) { AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_io_message(errno, line, column) } } }; return out; }
    ae_string failure = { NULL, 0 };
    if (fseek(f, 0, SEEK_END) != 0) failure = ae_io_message(errno, line, column);
    long size = failure.ptr ? -1 : ftell(f);
    if (!failure.ptr && size < 0) failure = ae_io_message(errno, line, column);
    if (!failure.ptr) rewind(f);
    char *buffer = NULL;
    if (!failure.ptr) {
        buffer = malloc((size_t)size + 1);
        if (!buffer) failure = ae_str_from("out of memory", line, column);
    }
    if (!failure.ptr) {
        const size_t read = fread(buffer, 1, (size_t)size, f);
        if (read != (size_t)size) { free(buffer); buffer = NULL; failure = ae_io_message(errno, line, column); }
    }
    fclose(f);
    if (failure.ptr) { AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { failure } } }; return out; }
    buffer[size] = 0;
    ae_string content = { buffer, (int64_t)size };
    AE_READ_RESULT out = { AE_READ_OK, { .v0 = { content } } };
    return out;
}

static AE_WRITE_RESULT ae_try_write_file(const char *path, ae_strview content, size_t line, size_t column) {
    errno = 0;
    FILE *f = ae_fopen(path, "wb");
    if (!f) { AE_WRITE_RESULT out = { AE_WRITE_ERR, { .v1 = { ae_io_message(errno, line, column) } } }; return out; }
    const size_t written = fwrite(content.ptr, 1, (size_t)content.len, f);
    const bool closed = fclose(f) == 0;
    if (!closed || written != (size_t)content.len) {
        AE_WRITE_RESULT out = { AE_WRITE_ERR, { .v1 = { ae_io_message(errno, line, column) } } };
        return out;
    }
    AE_WRITE_RESULT out = { AE_WRITE_OK, { .v0 = { (int64_t)written } } };
    return out;
}

// Strict integer parsing: optional sign, ASCII digits only, whole string, and
// no wrap-around. Anything else is an error value, never a silent default.
static AE_INT_RESULT ae_parse_int(ae_strview text, size_t line, size_t column) {
    int64_t at = 0;
    bool negative = false;
    if (text.len > 0 && (text.ptr[0] == '-' || text.ptr[0] == '+')) { negative = text.ptr[0] == '-'; at = 1; }
    if (at >= text.len) {
        AE_INT_RESULT bad = { AE_INT_ERR, { .v1 = { ae_str_from("empty or sign-only integer", line, column) } } };
        return bad;
    }
    uint64_t magnitude = 0;
    const uint64_t limit = negative ? (uint64_t)INT64_MAX + 1u : (uint64_t)INT64_MAX;
    for (; at < text.len; at++) {
        const char c = text.ptr[at];
        if (c < '0' || c > '9') {
            AE_INT_RESULT bad = { AE_INT_ERR, { .v1 = { ae_str_from("not a base-10 integer", line, column) } } };
            return bad;
        }
        const uint64_t digit = (uint64_t)(c - '0');
        if (magnitude > (limit - digit) / 10u) {
            AE_INT_RESULT bad = { AE_INT_ERR, { .v1 = { ae_str_from("integer out of i64 range", line, column) } } };
            return bad;
        }
        magnitude = magnitude * 10u + digit;
    }
    const int64_t value = negative ? (int64_t)(0u - magnitude) : (int64_t)magnitude;
    AE_INT_RESULT out = { AE_INT_OK, { .v0 = { value } } };
    return out;
}

static ae_string ae_to_string(int64_t value, size_t line, size_t column) {
    char buffer[24];
    const int length = snprintf(buffer, sizeof buffer, "%" PRId64, value);
    if (length < 0) ae_fail("cannot format integer", line, column);
    return ae_str_from_len(buffer, (int64_t)length, line, column);
}

static bool ae_path_exists(const char *path, size_t line, size_t column) {
    (void)line; (void)column;
#ifdef _WIN32
    wchar_t *wide = ae_widen(path);
    if (!wide) return false;
    const DWORD attributes = GetFileAttributesW(wide);
    free(wide);
    return attributes != INVALID_FILE_ATTRIBUTES;
#else
    struct stat info;
    return stat(path, &info) == 0;
#endif
}

static bool ae_path_is_dir(const char *path, size_t line, size_t column) {
    (void)line; (void)column;
#ifdef _WIN32
    wchar_t *wide = ae_widen(path);
    if (!wide) return false;
    const DWORD attributes = GetFileAttributesW(wide);
    free(wide);
    return attributes != INVALID_FILE_ATTRIBUTES && (attributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
#else
    struct stat info;
    return stat(path, &info) == 0 && S_ISDIR(info.st_mode);
#endif
}

// Directory listing: entry names only, never "." or "..", sorted by bytes so
// repeated runs and the two platforms agree on order.
static int ae_name_compare(const void *left, const void *right) {
    const ae_string *a = left;
    const ae_string *b = right;
    const int64_t shared = a->len < b->len ? a->len : b->len;
    const int order = shared > 0 ? memcmp(a->ptr, b->ptr, (size_t)shared) : 0;
    if (order != 0) return order;
    return a->len < b->len ? -1 : (a->len > b->len ? 1 : 0);
}
static AE_DIR_RESULT ae_try_list_dir(const char *path, size_t line, size_t column) {
    ae_strvec names = ae_strvec_new(line, column);
#ifdef _WIN32
    const size_t length = strlen(path);
    const bool separated = length > 0 && (path[length - 1] == '\\' || path[length - 1] == '/');
    char *pattern = malloc(length + (separated ? 2 : 3));
    if (!pattern) {
        ae_strvec_release(&names);
        AE_DIR_RESULT out = { AE_DIR_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    memcpy(pattern, path, length);
    if (!separated) pattern[length] = '\\';
    pattern[length + (separated ? 0 : 1)] = '*';
    pattern[length + (separated ? 1 : 2)] = 0;
    wchar_t *wide = ae_widen(pattern);
    free(pattern);
    if (!wide) {
        ae_strvec_release(&names);
        AE_DIR_RESULT out = { AE_DIR_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    WIN32_FIND_DATAW data;
    HANDLE walk = FindFirstFileW(wide, &data);
    free(wide);
    if (walk == INVALID_HANDLE_VALUE) {
        ae_string message = ae_status_message("cannot list directory", (unsigned long)GetLastError(), line, column);
        ae_strvec_release(&names);
        AE_DIR_RESULT out = { AE_DIR_ERR, { .v1 = { message } } };
        return out;
    }
    do {
        if (wcscmp(data.cFileName, L".") == 0 || wcscmp(data.cFileName, L"..") == 0) continue;
        char *utf8 = ae_narrow(data.cFileName, (int)wcslen(data.cFileName));
        if (!utf8) {
            FindClose(walk);
            ae_strvec_release(&names);
            AE_DIR_RESULT out = { AE_DIR_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
            return out;
        }
        ae_strvec_push(&names, ae_str_from(utf8, line, column), line, column);
        free(utf8);
    } while (FindNextFileW(walk, &data));
    FindClose(walk);
#else
    DIR *walk = opendir(path);
    if (!walk) {
        ae_string message = ae_io_message(errno, line, column);
        ae_strvec_release(&names);
        AE_DIR_RESULT out = { AE_DIR_ERR, { .v1 = { message } } };
        return out;
    }
    struct dirent *entry;
    while ((entry = readdir(walk)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) continue;
        ae_strvec_push(&names, ae_str_from(entry->d_name, line, column), line, column);
    }
    closedir(walk);
#endif
    if (names.len > 1) qsort(names.data, (size_t)names.len, sizeof(ae_string), ae_name_compare);
    AE_DIR_RESULT out = { AE_DIR_OK, { .v0 = { names } } };
    return out;
}

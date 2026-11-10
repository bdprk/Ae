// Fallible built-ins and platform helpers that name prelude enum types.
// Included after the enum layouts, so ae_e0..ae_e3 exist here.
// Invariant (checked by the emitter): the prelude registers exactly
//   0 ReadResult{Ok(string), Err(string)}
//   1 WriteResult{Ok(i64), Err(string)}
//   2 IntResult{Ok(i64), Err(string)}
//   3 DirResult{Ok(vec[string]), Err(string)}
// with Ok at variant index 0 and Err at index 1.

#include <errno.h>
#include <time.h>
#ifndef _WIN32
#include <dirent.h>
#include <sys/stat.h>
#include <unistd.h>
#else
#include <io.h>
#include <fcntl.h>
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
#define AE_FILE_RESULT ae_e4
#define AE_FILE_OK AE_E4_0
#define AE_FILE_ERR AE_E4_1
#define AE_STATUS_RESULT ae_e5
#define AE_STATUS_OK AE_E5_0
#define AE_STATUS_ERR AE_E5_1
#define AE_BOOL_RESULT ae_e6
#define AE_BOOL_OK AE_E6_0
#define AE_BOOL_ERR AE_E6_1

static AE_STATUS_RESULT ae_status_ok(void) {
    AE_STATUS_RESULT out = { AE_STATUS_OK, { { 0 } } };
    return out;
}
static AE_STATUS_RESULT ae_status_failed(ae_string message) {
    AE_STATUS_RESULT out = { AE_STATUS_ERR, { .v1 = { message } } };
    return out;
}

// Wall-clock time in milliseconds since the Unix epoch. File metadata reports
// whole seconds, so now_s exists to compare against it without dividing on
// every call. Both are i64: the epoch range fits comfortably, and negative
// values (before 1970) stay meaningful.
static int64_t ae_now_ms(void) {
#ifdef _WIN32
    FILETIME ft;
    GetSystemTimeAsFileTime(&ft);
    const int64_t ticks = ((int64_t)ft.dwHighDateTime << 32) | (int64_t)ft.dwLowDateTime;
    // 100-nanosecond ticks since 1601 -> milliseconds since 1970.
    return ticks / 10000 - 11644473600000LL;
#else
    // timespec_get is ISO C11, so the generated file still compiles under
    // -std=c11 -pedantic-errors without defining a feature-test macro;
    // clock_gettime would need _POSIX_C_SOURCE and is hidden by strict mode.
    struct timespec ts;
    if (timespec_get(&ts, TIME_UTC) != TIME_UTC) return 0;
    return (int64_t)ts.tv_sec * 1000 + (int64_t)(ts.tv_nsec / 1000000);
#endif
}
static int64_t ae_now_s(void) {
    const int64_t ms = ae_now_ms();
    // Floor division, so a negative timestamp still rounds consistently.
    return ms / 1000 - (ms % 1000 < 0 ? 1 : 0);
}

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
        buffer = ae_alloc_try((size_t)size + 1);
        if (!buffer) failure = ae_str_from("out of memory", line, column);
    }
    if (!failure.ptr) {
        const size_t read = fread(buffer, 1, (size_t)size, f);
        if (read != (size_t)size) { ae_free(buffer); buffer = NULL; failure = ae_io_message(errno, line, column); }
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

// Appending exists as its own operation because "read, concatenate, write" is
// not the same thing: it rewrites the whole file, loses the atomicity of a
// single append, and fails on files the process may only append to.
static AE_WRITE_RESULT ae_try_append_file(const char *path, ae_strview content, size_t line, size_t column) {
    errno = 0;
    FILE *f = ae_fopen(path, "ab");
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

// Environment lookup. A variable that is set but empty is Ok(""), a variable
// that does not exist is Err: collapsing the two would silently turn "PATH is
// empty" into "PATH is missing".
static AE_READ_RESULT ae_try_env(const char *name, size_t line, size_t column) {
    if (name[0] == 0) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("variable name is empty", line, column) } } };
        return out;
    }
#ifdef _WIN32
    wchar_t *wname = ae_widen(name);
    if (!wname) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    // A variable whose value is empty also reports 0, so the last error is
    // cleared first: ERROR_SUCCESS afterwards means "set but empty", and only
    // ERROR_ENVVAR_NOT_FOUND means the name is absent.
    SetLastError(ERROR_SUCCESS);
    const DWORD needed = GetEnvironmentVariableW(wname, NULL, 0);
    if (needed == 0) {
        const DWORD code = GetLastError();
        ae_free(wname);
        if (code == ERROR_SUCCESS) {
            AE_READ_RESULT empty = { AE_READ_OK, { .v0 = { ae_str_from("", line, column) } } };
            return empty;
        }
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { code == ERROR_ENVVAR_NOT_FOUND ? ae_str_from("variable not set", line, column) : ae_status_message("cannot read variable", code, line, column) } } };
        return out;
    }
    wchar_t *buffer = ae_alloc_try((size_t)needed * sizeof(wchar_t));
    if (!buffer) {
        ae_free(wname);
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    SetLastError(ERROR_SUCCESS);
    const DWORD length = GetEnvironmentVariableW(wname, buffer, needed);
    const DWORD read_code = GetLastError();
    ae_free(wname);
    if (length >= needed) {
        ae_free(buffer);
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("variable changed while reading", line, column) } } };
        return out;
    }
    if (length == 0 && read_code != ERROR_SUCCESS) {
        ae_free(buffer);
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_status_message("cannot read variable", read_code, line, column) } } };
        return out;
    }
    // An empty value is a successful read of zero characters: narrow() rejects
    // a zero length, so build the empty string directly.
    ae_string value = length == 0 ? ae_str_from("", line, column) : (ae_string){ NULL, 0 };
    if (length > 0) {
        char *utf8 = ae_narrow(buffer, (int)length);
        if (!utf8) {
            ae_free(buffer);
            AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
            return out;
        }
        value = ae_str_from(utf8, line, column);
        ae_free(utf8);
    }
    ae_free(buffer);
    AE_READ_RESULT out = { AE_READ_OK, { .v0 = { value } } };
    return out;
#else
    (void)line; (void)column;
    const char *value = getenv(name);
    if (!value) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("variable not set", line, column) } } };
        return out;
    }
    AE_READ_RESULT out = { AE_READ_OK, { .v0 = { ae_str_from(value, line, column) } } };
    return out;
#endif
}

// Everything on standard input, read as bytes. Windows starts in text mode,
// which would turn CRLF into LF and make the byte count differ between
// platforms, so the descriptor is switched to binary first. The buffer grows by
// doubling; a fully read pipe returns however much arrived, which is not an
// error -- "no input" and "failed to read input" must stay distinguishable.
static AE_READ_RESULT ae_try_read_stdin(size_t line, size_t column) {
#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
#endif
    size_t capacity = 8192;
    size_t length = 0;
    char *buffer = ae_alloc_try(capacity);
    if (!buffer) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    errno = 0;
    for (;;) {
        if (length == capacity) {
            if (capacity > ((size_t)-1) / 2) {
                ae_free(buffer);
                AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("input too large", line, column) } } };
                return out;
            }
            char *grown = ae_alloc_try(capacity * 2);
            if (!grown) {
                ae_free(buffer);
                AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
                return out;
            }
            memcpy(grown, buffer, length);
            ae_free(buffer);
            buffer = grown;
            capacity = capacity * 2;
        }
        const size_t got = fread(buffer + length, 1, capacity - length, stdin);
        length = length + got;
        if (got == 0) break;
    }
    if (ferror(stdin)) {
        const int code = errno;
        ae_free(buffer);
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_io_message(code, line, column) } } };
        return out;
    }
    ae_string text = ae_str_from_len(buffer, (int64_t)length, line, column);
    ae_free(buffer);
    AE_READ_RESULT out = { AE_READ_OK, { .v0 = { text } } };
    return out;
}

// Current working directory, in the same `/`-separated form the rest of the
// language uses for paths.
static AE_READ_RESULT ae_try_cwd(size_t line, size_t column) {
#ifdef _WIN32
    const DWORD needed = GetCurrentDirectoryW(0, NULL);
    if (needed == 0) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_status_message("cannot read working directory", (unsigned long)GetLastError(), line, column) } } };
        return out;
    }
    wchar_t *buffer = ae_alloc_try((size_t)needed * sizeof(wchar_t));
    if (!buffer) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    const DWORD length = GetCurrentDirectoryW(needed, buffer);
    if (length == 0 || length >= needed) {
        ae_free(buffer);
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_status_message("cannot read working directory", (unsigned long)GetLastError(), line, column) } } };
        return out;
    }
    char *utf8 = ae_narrow(buffer, (int)length);
    ae_free(buffer);
    if (!utf8) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    ae_string raw = ae_str_from(utf8, line, column);
    ae_free(utf8);
    // Unify separators so path_join and friends see one form.
    for (int64_t i = 0; i < raw.len; i++) if (raw.ptr[i] == '\\') raw.ptr[i] = '/';
    AE_READ_RESULT out = { AE_READ_OK, { .v0 = { raw } } };
    return out;
#else
    size_t capacity = 256;
    char *buffer = ae_alloc_try(capacity);
    if (!buffer) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    while (!getcwd(buffer, capacity)) {
        if (errno != ERANGE || capacity > ((size_t)-1) / 2) {
            const int code = errno;
            ae_free(buffer);
            AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_io_message(code, line, column) } } };
            return out;
        }
        char *grown = ae_alloc_try(capacity * 2);
        if (!grown) {
            ae_free(buffer);
            AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
            return out;
        }
        ae_free(buffer);
        buffer = grown;
        capacity = capacity * 2;
    }
    ae_string text = ae_str_from(buffer, line, column);
    ae_free(buffer);
    AE_READ_RESULT out = { AE_READ_OK, { .v0 = { text } } };
    return out;
#endif
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
    ae_free(wide);
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
    ae_free(wide);
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
// In-place orderings for owned string vectors, byte-order like keys() and
// directory listings so every sorted output is comparable across platforms.
static void ae_strvec_sort(ae_strvec *v) {
    if (v->len > 1) qsort(v->data, (size_t)v->len, sizeof(ae_string), ae_name_compare);
}
static void ae_strvec_reverse(ae_strvec *v) {
    for (int64_t i = 0, j = v->len - 1; i < j; i = i + 1, j = j - 1) {
        const ae_string swap = v->data[i];
        v->data[i] = v->data[j];
        v->data[j] = swap;
    }
}
static AE_DIR_RESULT ae_try_list_dir(const char *path, size_t line, size_t column) {
    ae_strvec names = ae_strvec_new(line, column);
#ifdef _WIN32
    const size_t length = strlen(path);
    const bool separated = length > 0 && (path[length - 1] == '\\' || path[length - 1] == '/');
    char *pattern = ae_alloc_try(length + (separated ? 2 : 3));
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
    ae_free(pattern);
    if (!wide) {
        ae_strvec_release(&names);
        AE_DIR_RESULT out = { AE_DIR_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    WIN32_FIND_DATAW data;
    HANDLE walk = FindFirstFileW(wide, &data);
    ae_free(wide);
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
        ae_free(utf8);
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

// ---- Path algebra -----------------------------------------------------------
// Separators: both '/' and '\' are accepted on input, but '/'-joined output is
// what this layer emits, because Windows accepts '/' and identical output on
// both platforms keeps tests and recorded results comparable.

static bool ae_is_separator(char c) { return c == '/' || c == '\\'; }

// Concatenate `dir` and `name`, inserting a separator only when needed.
static ae_string ae_path_concat(ae_strview dir, ae_strview name, bool separator, size_t line, size_t column) {
    const int64_t total = dir.len + name.len + (separator ? 1 : 0);
    char *buffer = ae_alloc((size_t)total + 1, line, column);
    memcpy(buffer, dir.ptr, (size_t)dir.len);
    int64_t at = dir.len;
    if (separator) buffer[at++] = '/';
    memcpy(buffer + at, name.ptr, (size_t)name.len);
    buffer[total] = 0;
    ae_string out = { buffer, total };
    return out;
}
// An absolute name (leading separator, or a drive letter) replaces the
// directory instead of being appended to it.
static bool ae_path_is_absolute(ae_strview name) {
    if (name.len == 0) return false;
    if (ae_is_separator(name.ptr[0])) return true;
    return name.len >= 2 && name.ptr[1] == ':';
}
static ae_string ae_path_join(ae_strview dir, ae_strview name, size_t line, size_t column) {
    if (name.len == 0) return ae_str_copy_view(dir, line, column);
    if (dir.len == 0) return ae_str_copy_view(name, line, column);
    if (ae_path_is_absolute(name)) return ae_str_copy_view(name, line, column);
    const bool separated = ae_is_separator(dir.ptr[dir.len - 1]);
    return ae_path_concat(dir, name, !separated, line, column);
}
// Final component of a path. Trailing separators are dropped first so
// "dir/name/" yields "name"; a path that is only separators keeps one so the
// root stays recognisable.
static ae_string ae_path_name(ae_strview path, size_t line, size_t column) {
    int64_t end = path.len;
    while (end > 1 && ae_is_separator(path.ptr[end - 1])) end--;
    int64_t start = end;
    while (start > 0 && !ae_is_separator(path.ptr[start - 1])) start--;
    if (start == end) {
        // All separators: the name of the root is the root marker itself.
        ae_strview root = { path.ptr, end };
        return ae_str_copy_view(root, line, column);
    }
    ae_strview tail = { path.ptr + start, end - start };
    return ae_str_copy_view(tail, line, column);
}
// Everything before the final component. A path without a separator has no
// parent, which reports as the empty string rather than a fabricated "." .
static ae_string ae_path_parent(ae_strview path, size_t line, size_t column) {
    int64_t end = path.len;
    while (end > 1 && ae_is_separator(path.ptr[end - 1])) end--;
    int64_t at = end;
    while (at > 0 && !ae_is_separator(path.ptr[at - 1])) at--;
    if (at == 0) return ae_str_from_len("", 0, line, column);
    int64_t stop = at;
    while (stop > 1 && ae_is_separator(path.ptr[stop - 1])) stop--;
#ifdef _WIN32
    // Keep the separator after a drive prefix: "C:/dir" has parent "C:/".
    if (stop == 2 && path.ptr[1] == ':' && at > stop) stop = at;
#endif
    if (stop == 1 && ae_is_separator(path.ptr[0])) stop = 1;
    return ae_str_from_len(path.ptr, stop, line, column);
}

// ---- Metadata ---------------------------------------------------------------
// Size in bytes, modification time in seconds since the Unix epoch, and whether
// the target is a directory. Failure is a value, never a silent zero size.
static AE_FILE_RESULT ae_try_file_info(const char *path, size_t line, size_t column) {
#ifdef _WIN32
    wchar_t *wide = ae_widen(path);
    if (!wide) {
        AE_FILE_RESULT out = { AE_FILE_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    WIN32_FILE_ATTRIBUTE_DATA data;
    if (!GetFileAttributesExW(wide, GetFileExInfoStandard, &data)) {
        const unsigned long status = (unsigned long)GetLastError();
        ae_free(wide);
        AE_FILE_RESULT out = { AE_FILE_ERR, { .v1 = { ae_status_message("cannot read file metadata", status, line, column) } } };
        return out;
    }
    ae_free(wide);
    const int64_t size = (int64_t)(((uint64_t)data.nFileSizeHigh << 32) | (uint64_t)data.nFileSizeLow);
    // FILETIME counts 100 ns ticks from 1601-01-01; this offset lands on the
    // Unix epoch, so the number matches st_mtime on POSIX.
    const int64_t ticks = (int64_t)(((uint64_t)data.ftLastWriteTime.dwHighDateTime << 32) | (uint64_t)data.ftLastWriteTime.dwLowDateTime);
    const int64_t modified = ticks / 10000000 - 11644473600LL;
    const bool is_dir = (data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
    AE_FILE_RESULT out = { AE_FILE_OK, { .v0 = { size, modified, is_dir } } };
    return out;
#else
    errno = 0;
    struct stat info;
    if (stat(path, &info) != 0) {
        AE_FILE_RESULT out = { AE_FILE_ERR, { .v1 = { ae_io_message(errno, line, column) } } };
        return out;
    }
    AE_FILE_RESULT out = { AE_FILE_OK, { .v0 = { (int64_t)info.st_size, (int64_t)info.st_mtime, S_ISDIR(info.st_mode) } } };
    return out;
#endif
}

// ---- Mutation --------------------------------------------------------------
static AE_STATUS_RESULT ae_try_copy_file(const char *from, const char *to, size_t line, size_t column) {
    errno = 0;
    FILE *source = ae_fopen(from, "rb");
    if (!source) return ae_status_failed(ae_io_message(errno, line, column));
    FILE *target = ae_fopen(to, "wb");
    if (!target) {
        fclose(source);
        return ae_status_failed(ae_io_message(errno, line, column));
    }
    ae_string failure = { NULL, 0 };
    char buffer[65536];
    size_t read;
    while ((read = fread(buffer, 1, sizeof buffer, source)) > 0) {
        if (fwrite(buffer, 1, read, target) != read) { failure = ae_io_message(errno, line, column); break; }
    }
    if (!failure.ptr && ferror(source)) failure = ae_io_message(errno, line, column);
    if (fclose(source) != 0 && !failure.ptr) failure = ae_io_message(errno, line, column);
    if (fclose(target) != 0 && !failure.ptr) failure = ae_io_message(errno, line, column);
    if (failure.ptr) return ae_status_failed(failure);
    return ae_status_ok();
}

static AE_STATUS_RESULT ae_try_rename_path(const char *from, const char *to, size_t line, size_t column) {
#ifdef _WIN32
    errno = 0;
    wchar_t *source = ae_widen(from);
    wchar_t *target = ae_widen(to);
    if (!source || !target) {
        ae_free(source); ae_free(target);
        return ae_status_failed(ae_str_from("out of memory", line, column));
    }
    const bool moved = MoveFileExW(source, target, MOVEFILE_REPLACE_EXISTING | MOVEFILE_COPY_ALLOWED) != 0;
    const unsigned long status = (unsigned long)GetLastError();
    ae_free(source); ae_free(target);
    if (!moved) return ae_status_failed(ae_status_message("cannot rename path", status, line, column));
    return ae_status_ok();
#else
    errno = 0;
    if (rename(from, to) != 0) return ae_status_failed(ae_io_message(errno, line, column));
    return ae_status_ok();
#endif
}

// Removes a file. Directories are refused on both platforms: POSIX remove()
// would also delete an empty directory, and reporting the same outcome on two
// platforms matters more than the extra convenience.
static AE_STATUS_RESULT ae_try_remove_file(const char *path, size_t line, size_t column) {
    if (ae_path_is_dir(path, line, column)) {
        return ae_status_failed(ae_str_from("cannot remove a directory with try_remove_file", line, column));
    }
#ifdef _WIN32
    errno = 0;
    wchar_t *wide = ae_widen(path);
    if (!wide) return ae_status_failed(ae_str_from("out of memory", line, column));
    const bool removed = DeleteFileW(wide) != 0;
    const unsigned long status = (unsigned long)GetLastError();
    ae_free(wide);
    if (!removed) return ae_status_failed(ae_status_message("cannot remove file", status, line, column));
    return ae_status_ok();
#else
    errno = 0;
    if (remove(path) != 0) return ae_status_failed(ae_io_message(errno, line, column));
    return ae_status_ok();
#endif
}

// Returns an empty string on success, or the failure text. Keeping the message
// as the return value lets the caller decide whether a race is acceptable.
static ae_string ae_make_one_dir(const char *path, size_t line, size_t column) {
    ae_string none = { NULL, 0 };
#ifdef _WIN32
    wchar_t *wide = ae_widen(path);
    if (!wide) return ae_str_from("out of memory", line, column);
    if (CreateDirectoryW(wide, NULL)) { ae_free(wide); return none; }
    const unsigned long status = (unsigned long)GetLastError();
    ae_free(wide);
    if (status == ERROR_ALREADY_EXISTS) return none;
    return ae_status_message("cannot create directory", status, line, column);
#else
    errno = 0;
    if (mkdir(path, 0777) == 0) return none;
    if (errno == EEXIST) return none;
    return ae_io_message(errno, line, column);
#endif
}
// Creates the directory and every missing parent, so a caller can write
// "out/2026/march/" in one step. Existing directories are not an error.
static AE_STATUS_RESULT ae_try_create_dir(const char *path, size_t line, size_t column) {
    const int64_t length = (int64_t)strlen(path);
    if (length == 0) return ae_status_failed(ae_str_from("empty path", line, column));
    char *buffer = ae_alloc_try((size_t)length + 1);
    if (!buffer) return ae_status_failed(ae_str_from("out of memory", line, column));
    memcpy(buffer, path, (size_t)length + 1);
    ae_string failure = { NULL, 0 };
    for (int64_t at = 1; at <= length; at++) {
        if (at < length && !ae_is_separator(buffer[at])) continue;
        const char saved = buffer[at];
        buffer[at] = 0;
        bool skip = false;
#ifdef _WIN32
        if (at == 2 && buffer[1] == ':') skip = true; // drive prefix
#endif
        if (!skip && !ae_path_is_dir(buffer, line, column)) {
            failure = ae_make_one_dir(buffer, line, column);
        }
        buffer[at] = saved;
        if (failure.ptr) break;
    }
    ae_free(buffer);
    if (failure.ptr) return ae_status_failed(failure);
    return ae_status_ok();
}

// Removes an empty directory. Recursive removal is deliberately absent: a
// helper that silently deletes a tree is the easiest way to lose data.
static AE_STATUS_RESULT ae_try_remove_dir(const char *path, size_t line, size_t column) {
#ifdef _WIN32
    wchar_t *wide = ae_widen(path);
    if (!wide) return ae_status_failed(ae_str_from("out of memory", line, column));
    const bool removed = RemoveDirectoryW(wide) != 0;
    const unsigned long status = (unsigned long)GetLastError();
    ae_free(wide);
    if (!removed) return ae_status_failed(ae_status_message("cannot remove directory", status, line, column));
    return ae_status_ok();
#else
    errno = 0;
    if (rmdir(path) != 0) return ae_status_failed(ae_io_message(errno, line, column));
    return ae_status_ok();
#endif
}

// ---- Map helpers that need the prelude result types -------------------------
// A missing key is a value, not a zero: `0` is a perfectly normal count.
static AE_INT_RESULT ae_map_get(const ae_map *m, ae_strview key, size_t line, size_t column) {
    int64_t value = 0;
    if (!ae_map_lookup(m, key, &value)) {
        AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from("key not found", line, column) } } };
        return out;
    }
    AE_INT_RESULT out = { AE_INT_OK, { .v0 = { value } } };
    return out;
}
// Keys are copied and sorted by bytes, so iteration is reproducible and the
// caller owns a plain string vector.
static ae_strvec ae_map_keys(const ae_map *m, size_t line, size_t column) {
    ae_strvec keys = ae_strvec_new(line, column);
    for (int64_t i = 0; i < m->cap; i++) {
        if (m->slots[i].state != AE_MAP_FULL) continue;
        ae_strvec_push(&keys, ae_str_copy_view(AE_VIEW(m->slots[i].key), line, column), line, column);
    }
    if (keys.len > 1) qsort(keys.data, (size_t)keys.len, sizeof(ae_string), ae_name_compare);
    return keys;
}

// ---- JSON reader (ADR-26) ---------------------------------------------------
// A path query, not a document model: one call selects one value and reports it
// as text, an integer or a boolean. The document is rescanned per call, which is
// the right trade for config-sized input and keeps JSON out of the type system:
// there is no container type to own, borrow or release.
#define AE_JSON_OK 0
#define AE_JSON_SYNTAX 1
#define AE_JSON_MISSING 2
#define AE_JSON_EXPECTED 3
#define AE_JSON_DEPTH 64
#define AE_JSON_STRING 0
#define AE_JSON_NUMBER 1
#define AE_JSON_BOOL 2
#define AE_JSON_NULL 3
#define AE_JSON_ARRAY 4
#define AE_JSON_OBJECT 5

static void ae_json_ws(const char *t, int64_t n, int64_t *i) {
    while (*i < n) {
        const char c = t[*i];
        if (c != ' ' && c != '\t' && c != '\n' && c != '\r') break;
        *i += 1;
    }
}
static int ae_json_hex(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}
static int ae_json_utf8(char *out, unsigned cp) {
    if (cp < 0x80u) { out[0] = (char)cp; return 1; }
    if (cp < 0x800u) { out[0] = (char)(0xC0u | (cp >> 6)); out[1] = (char)(0x80u | (cp & 0x3Fu)); return 2; }
    if (cp < 0x10000u) { out[0] = (char)(0xE0u | (cp >> 12)); out[1] = (char)(0x80u | ((cp >> 6) & 0x3Fu)); out[2] = (char)(0x80u | (cp & 0x3Fu)); return 3; }
    out[0] = (char)(0xF0u | (cp >> 18)); out[1] = (char)(0x80u | ((cp >> 12) & 0x3Fu)); out[2] = (char)(0x80u | ((cp >> 6) & 0x3Fu)); out[3] = (char)(0x80u | (cp & 0x3Fu)); return 4;
}
static int ae_json_skip_string(const char *t, int64_t n, int64_t *i) {
    if (*i >= n || t[*i] != '"') return 0;
    int64_t j = *i + 1;
    while (j < n) {
        const unsigned char c = (unsigned char)t[j];
        if (c == '"') { *i = j + 1; return 1; }
        if (c == '\\') { j += 2; continue; }
        if (c < 0x20u) return 0;
        j += 1;
    }
    return 0;
}
static int ae_json_skip_value(const char *t, int64_t n, int64_t *i, int depth) {
    if (depth > AE_JSON_DEPTH) return 0;
    ae_json_ws(t, n, i);
    if (*i >= n) return 0;
    const char c = t[*i];
    if (c == '"') return ae_json_skip_string(t, n, i);
    if (c == '{' || c == '[') {
        const char close = c == '{' ? '}' : ']';
        *i += 1;
        ae_json_ws(t, n, i);
        if (*i < n && t[*i] == close) { *i += 1; return 1; }
        for (;;) {
            ae_json_ws(t, n, i);
            if (c == '{') {
                if (!ae_json_skip_string(t, n, i)) return 0;
                ae_json_ws(t, n, i);
                if (*i >= n || t[*i] != ':') return 0;
                *i += 1;
            }
            if (!ae_json_skip_value(t, n, i, depth + 1)) return 0;
            ae_json_ws(t, n, i);
            if (*i >= n) return 0;
            if (t[*i] == ',') { *i += 1; continue; }
            if (t[*i] == close) { *i += 1; return 1; }
            return 0;
        }
    }
    // A bare token: number, true, false or null. Its end is punctuation or space.
    const int64_t start = *i;
    while (*i < n) {
        const char d = t[*i];
        if (d == ',' || d == '}' || d == ']' || d == ' ' || d == '\t' || d == '\r' || d == '\n') break;
        *i += 1;
    }
    return *i > start;
}
// Decodes one JSON string at *i into an owned string, resolving escapes (and
// surrogate pairs) so the language never sees a \uXXXX sequence.
static int ae_json_decode(const char *t, int64_t n, int64_t *i, ae_string *out, size_t line, size_t column) {
    if (*i >= n || t[*i] != '"') return 0;
    int64_t j = *i + 1;
    while (j < n) {
        const unsigned char c = (unsigned char)t[j];
        if (c == '"') break;
        if (c == '\\') { j += 2; continue; }
        if (c < 0x20u) return 0;
        j += 1;
    }
    if (j >= n) return 0;
    const int64_t raw = j - (*i + 1);
    char *buffer = ae_alloc((size_t)raw + 1, line, column);
    int64_t s = *i + 1;
    int64_t k = 0;
    while (s < j) {
        const unsigned char c = (unsigned char)t[s];
        if (c != '\\') { buffer[k] = (char)c; k += 1; s += 1; continue; }
        s += 1;
        if (s >= j) { ae_free(buffer); return 0; }
        const char e = t[s];
        s += 1;
        switch (e) {
            case '"': buffer[k] = '"'; k += 1; break;
            case '\\': buffer[k] = '\\'; k += 1; break;
            case '/': buffer[k] = '/'; k += 1; break;
            case 'b': buffer[k] = '\b'; k += 1; break;
            case 'f': buffer[k] = '\f'; k += 1; break;
            case 'n': buffer[k] = '\n'; k += 1; break;
            case 'r': buffer[k] = '\r'; k += 1; break;
            case 't': buffer[k] = '\t'; k += 1; break;
            case 'u': {
                unsigned cp = 0;
                if (s + 4 > j) { ae_free(buffer); return 0; }
                for (int d = 0; d < 4; d++) {
                    const int v = ae_json_hex(t[s + d]);
                    if (v < 0) { ae_free(buffer); return 0; }
                    cp = (cp << 4) | (unsigned)v;
                }
                s += 4;
                if (cp >= 0xD800u && cp <= 0xDBFFu && s + 6 <= j && t[s] == '\\' && t[s + 1] == 'u') {
                    unsigned low = 0;
                    int ok = 1;
                    for (int d = 0; d < 4; d++) {
                        const int v = ae_json_hex(t[s + 2 + d]);
                        if (v < 0) { ok = 0; break; }
                        low = (low << 4) | (unsigned)v;
                    }
                    if (ok && low >= 0xDC00u && low <= 0xDFFFu) {
                        cp = 0x10000u + ((cp - 0xD800u) << 10) + (low - 0xDC00u);
                        s += 6;
                    }
                }
                // A NUL would end the string early for every C consumer.
                if (cp == 0) { ae_free(buffer); return 0; }
                k += ae_json_utf8(buffer + k, cp);
                break;
            }
            default: ae_free(buffer); return 0;
        }
    }
    buffer[k] = 0;
    out->ptr = buffer;
    out->len = k;
    *i = j + 1;
    return 1;
}
static int ae_json_kind(const char *t, int64_t n, int64_t start) {
    if (start >= n) return -1;
    const char c = t[start];
    if (c == '"') return AE_JSON_STRING;
    if (c == '{') return AE_JSON_OBJECT;
    if (c == '[') return AE_JSON_ARRAY;
    if (c == 't' || c == 'f') return AE_JSON_BOOL;
    if (c == 'n') return AE_JSON_NULL;
    return AE_JSON_NUMBER;
}
static const char *ae_json_kind_name(int kind) {
    switch (kind) {
        case AE_JSON_STRING: return "a string";
        case AE_JSON_NUMBER: return "a number";
        case AE_JSON_BOOL: return "a boolean";
        case AE_JSON_NULL: return "null";
        case AE_JSON_ARRAY: return "an array";
        case AE_JSON_OBJECT: return "an object";
        default: return "nothing";
    }
}
// Locates one member inside the object at *pos, leaving its value span in
// *vs/*ve. *found separates "no such member" from "not an object at all".
static int ae_json_member(const char *t, int64_t n, int64_t *pos, const char *key, int64_t key_len, int64_t *vs, int64_t *ve, int *found, size_t line, size_t column) {
    *found = 0;
    int64_t i = *pos;
    if (i >= n || t[i] != '{') return 0;
    i += 1;
    ae_json_ws(t, n, &i);
    if (i < n && t[i] == '}') { *pos = i + 1; return 1; }
    for (;;) {
        ae_json_ws(t, n, &i);
        ae_string name;
        if (!ae_json_decode(t, n, &i, &name, line, column)) return 0;
        ae_json_ws(t, n, &i);
        if (i >= n || t[i] != ':') { ae_free(name.ptr); return 0; }
        i += 1;
        ae_json_ws(t, n, &i);
        const int64_t start = i;
        if (!ae_json_skip_value(t, n, &i, 0)) { ae_free(name.ptr); return 0; }
        const int match = name.len == key_len && memcmp(name.ptr, key, (size_t)key_len) == 0;
        ae_free(name.ptr);
        if (match) { *vs = start; *ve = i; *found = 1; return 1; }
        ae_json_ws(t, n, &i);
        if (i >= n) return 0;
        if (t[i] == ',') { i += 1; continue; }
        if (t[i] == '}') { *pos = i + 1; return 1; }
        return 0;
    }
}
static int ae_json_element(const char *t, int64_t n, int64_t *pos, int64_t index, int64_t *vs, int64_t *ve, int *found) {
    *found = 0;
    int64_t i = *pos;
    if (i >= n || t[i] != '[') return 0;
    i += 1;
    ae_json_ws(t, n, &i);
    if (i < n && t[i] == ']') { *pos = i + 1; return 1; }
    int64_t k = 0;
    for (;;) {
        ae_json_ws(t, n, &i);
        const int64_t start = i;
        if (!ae_json_skip_value(t, n, &i, 0)) return 0;
        if (k == index) { *vs = start; *ve = i; *found = 1; return 1; }
        k += 1;
        ae_json_ws(t, n, &i);
        if (i >= n) return 0;
        if (t[i] == ',') { i += 1; continue; }
        if (t[i] == ']') { *pos = i + 1; return 1; }
        return 0;
    }
}
// Validates the path on its own first, so a typo in the path is reported as a
// path error even when the document would have failed on the same segment.
static int ae_json_path_check(const char *path, int64_t plen, char *why, size_t why_cap) {
    int64_t p = 0;
    while (p < plen) {
        if (path[p] == '[') {
            const int64_t open = p;
            p += 1;
            int digits = 0;
            while (p < plen && path[p] >= '0' && path[p] <= '9') { p += 1; digits += 1; }
            if (digits == 0 || p >= plen || path[p] != ']') {
                snprintf(why, why_cap, "path index at byte %lld needs digits and a closing ']'", (long long)open);
                return 0;
            }
            p += 1;
            if (p < plen && path[p] == '.') {
                p += 1;
                if (p >= plen) { snprintf(why, why_cap, "path ends with '.'"); return 0; }
            }
            continue;
        }
        const int64_t kstart = p;
        while (p < plen && path[p] != '.' && path[p] != '[') p += 1;
        if (p == kstart) {
            snprintf(why, why_cap, "path has an empty segment at byte %lld", (long long)kstart);
            return 0;
        }
        if (p < plen && path[p] == '.') {
            p += 1;
            if (p >= plen) { snprintf(why, why_cap, "path ends with '.'"); return 0; }
        }
    }
    return 1;
}
// Path grammar: `key`, `key.sub`, `key[2].sub`, `[0].key`. A segment is a run of
// bytes up to the next '.' or '['; keys are compared after unescaping.
static int ae_json_locate(const char *t, int64_t n, const char *path, int64_t plen, int64_t *vs, int64_t *ve, int *kind, char *why, size_t why_cap, size_t line, size_t column) {
    if (!ae_json_path_check(path, plen, why, why_cap)) return AE_JSON_SYNTAX;
    int64_t pos = 0;
    ae_json_ws(t, n, &pos);
    const int64_t start0 = pos;
    if (!ae_json_skip_value(t, n, &pos, 0)) {
        snprintf(why, why_cap, "JSON syntax error near byte %lld", (long long)start0);
        return AE_JSON_SYNTAX;
    }
    int64_t start = start0;
    int64_t end = pos;
    int current = ae_json_kind(t, n, start);
    int64_t p = 0;
    while (p < plen) {
        if (path[p] == '[') {
            const int64_t open = p;
            p += 1;
            int64_t index = 0;
            int digits = 0;
            while (p < plen && path[p] >= '0' && path[p] <= '9' && digits < 10) { index = index * 10 + (path[p] - '0'); p += 1; digits += 1; }
            if (digits == 0 || p >= plen || path[p] != ']') {
                snprintf(why, why_cap, "path index at byte %lld needs digits and a closing ']'", (long long)open);
                return AE_JSON_SYNTAX;
            }
            p += 1;
            if (current != AE_JSON_ARRAY) {
                snprintf(why, why_cap, "path segment [%lld] needs an array, found %s", (long long)index, ae_json_kind_name(current));
                return AE_JSON_EXPECTED;
            }
            int64_t cursor = start;
            int found = 0;
            if (!ae_json_element(t, n, &cursor, index, &start, &end, &found)) {
                snprintf(why, why_cap, "JSON syntax error while reading an array element");
                return AE_JSON_SYNTAX;
            }
            if (!found) {
                snprintf(why, why_cap, "path segment [%lld] is past the end of the array", (long long)index);
                return AE_JSON_MISSING;
            }
        } else {
            const int64_t kstart = p;
            while (p < plen && path[p] != '.' && path[p] != '[') p += 1;
            const int64_t klen = p - kstart;
            if (klen == 0) {
                snprintf(why, why_cap, "path has an empty segment at byte %lld", (long long)kstart);
                return AE_JSON_SYNTAX;
            }
            if (current != AE_JSON_OBJECT) {
                snprintf(why, why_cap, "path segment '%.*s' needs an object, found %s", (int)klen, path + kstart, ae_json_kind_name(current));
                return AE_JSON_EXPECTED;
            }
            int64_t cursor = start;
            int found = 0;
            if (!ae_json_member(t, n, &cursor, path + kstart, klen, &start, &end, &found, line, column)) {
                snprintf(why, why_cap, "JSON syntax error while reading a member");
                return AE_JSON_SYNTAX;
            }
            if (!found) {
                snprintf(why, why_cap, "no member named '%.*s'", (int)klen, path + kstart);
                return AE_JSON_MISSING;
            }
        }
        current = ae_json_kind(t, n, start);
        if (p < plen) {
            if (path[p] != '.' && path[p] != '[') {
                snprintf(why, why_cap, "path segment ends unexpectedly at byte %lld", (long long)p);
                return AE_JSON_SYNTAX;
            }
            if (path[p] == '.') {
                p += 1;
                if (p >= plen) {
                    snprintf(why, why_cap, "path ends with '.'");
                    return AE_JSON_SYNTAX;
                }
            }
        }
    }
    *vs = start;
    *ve = end;
    *kind = current;
    return AE_JSON_OK;
}
static int ae_json_count(const char *t, int64_t n, int64_t start, int kind) {
    int64_t i = start + 1;
    int64_t count = 0;
    const char close = kind == AE_JSON_OBJECT ? '}' : ']';
    ae_json_ws(t, n, &i);
    if (i < n && t[i] == close) return 0;
    for (;;) {
        ae_json_ws(t, n, &i);
        if (kind == AE_JSON_OBJECT) {
            if (!ae_json_skip_string(t, n, &i)) return -1;
            ae_json_ws(t, n, &i);
            if (i >= n || t[i] != ':') return -1;
            i += 1;
        }
        if (!ae_json_skip_value(t, n, &i, 0)) return -1;
        count += 1;
        ae_json_ws(t, n, &i);
        if (i >= n) return -1;
        if (t[i] == ',') { i += 1; continue; }
        if (t[i] == close) return count;
        return -1;
    }
}
static AE_READ_RESULT ae_json_get(const char *doc_text, int64_t doc_size, const char *path_text, int64_t path_size, size_t line, size_t column) {
    const ae_strview doc = { doc_text, doc_size };
    const ae_strview path = { path_text, path_size };

    int64_t vs = 0;
    int64_t ve = 0;
    int kind = 0;
    char why[192];
    if (ae_json_locate(doc.ptr, doc.len, path.ptr, path.len, &vs, &ve, &kind, why, sizeof why, line, column) != AE_JSON_OK) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    ae_string value;
    if (kind == AE_JSON_STRING) {
        int64_t at = vs;
        if (!ae_json_decode(doc.ptr, doc.len, &at, &value, line, column)) {
            AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("JSON string could not be decoded", line, column) } } };
            return out;
        }
    } else {
        value = ae_str_from_len(doc.ptr + vs, ve - vs, line, column);
    }
    AE_READ_RESULT out = { AE_READ_OK, { .v0 = { value } } };
    return out;
}
static AE_INT_RESULT ae_json_int(const char *doc_text, int64_t doc_size, const char *path_text, int64_t path_size, size_t line, size_t column) {
    const ae_strview doc = { doc_text, doc_size };
    const ae_strview path = { path_text, path_size };

    int64_t vs = 0;
    int64_t ve = 0;
    int kind = 0;
    char why[192];
    if (ae_json_locate(doc.ptr, doc.len, path.ptr, path.len, &vs, &ve, &kind, why, sizeof why, line, column) != AE_JSON_OK) {
        AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    if (kind != AE_JSON_NUMBER) {
        snprintf(why, sizeof why, "'%.*s' is %s, not a number", (int)path.len, path.ptr, ae_json_kind_name(kind));
        AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    int64_t i = vs;
    int negative = 0;
    if (i < ve && doc.ptr[i] == '-') { negative = 1; i += 1; }
    if (i >= ve) {
        snprintf(why, sizeof why, "'%.*s' is not an integer", (int)path.len, path.ptr);
        AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    int64_t value = 0;
    for (; i < ve; i++) {
        const char d = doc.ptr[i];
        if (d < '0' || d > '9') {
            // A fraction or exponent is a real number; this reader has no float type.
            snprintf(why, sizeof why, "'%.*s' is not an integer", (int)path.len, path.ptr);
            AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from(why, line, column) } } };
            return out;
        }
        const int digit = d - '0';
        if (value > (INT64_MAX - digit) / 10) {
            snprintf(why, sizeof why, "'%.*s' does not fit in i64", (int)path.len, path.ptr);
            AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from(why, line, column) } } };
            return out;
        }
        value = value * 10 + digit;
    }
    AE_INT_RESULT out = { AE_INT_OK, { .v0 = { negative ? -value : value } } };
    return out;
}
static AE_BOOL_RESULT ae_json_bool(const char *doc_text, int64_t doc_size, const char *path_text, int64_t path_size, size_t line, size_t column) {
    const ae_strview doc = { doc_text, doc_size };
    const ae_strview path = { path_text, path_size };

    int64_t vs = 0;
    int64_t ve = 0;
    int kind = 0;
    char why[192];
    if (ae_json_locate(doc.ptr, doc.len, path.ptr, path.len, &vs, &ve, &kind, why, sizeof why, line, column) != AE_JSON_OK) {
        AE_BOOL_RESULT out = { AE_BOOL_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    const int64_t width = ve - vs;
    bool value = false;
    if (kind == AE_JSON_BOOL && width == 4 && memcmp(doc.ptr + vs, "true", 4) == 0) value = true;
    else if (!(kind == AE_JSON_BOOL && width == 5 && memcmp(doc.ptr + vs, "false", 5) == 0)) {
        snprintf(why, sizeof why, "'%.*s' is %s, not a boolean", (int)path.len, path.ptr, ae_json_kind_name(kind));
        AE_BOOL_RESULT out = { AE_BOOL_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    AE_BOOL_RESULT out = { AE_BOOL_OK, { .v0 = { value } } };
    return out;
}
static AE_INT_RESULT ae_json_len(const char *doc_text, int64_t doc_size, const char *path_text, int64_t path_size, size_t line, size_t column) {
    const ae_strview doc = { doc_text, doc_size };
    const ae_strview path = { path_text, path_size };

    int64_t vs = 0;
    int64_t ve = 0;
    int kind = 0;
    char why[192];
    if (ae_json_locate(doc.ptr, doc.len, path.ptr, path.len, &vs, &ve, &kind, why, sizeof why, line, column) != AE_JSON_OK) {
        AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    if (kind != AE_JSON_ARRAY && kind != AE_JSON_OBJECT) {
        snprintf(why, sizeof why, "'%.*s' is %s; only arrays and objects have a member count", (int)path.len, path.ptr, ae_json_kind_name(kind));
        AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    const int64_t count = ae_json_count(doc.ptr, doc.len, vs, kind);
    if (count < 0) {
        AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from("JSON syntax error while counting members", line, column) } } };
        return out;
    }
    AE_INT_RESULT out = { AE_INT_OK, { .v0 = { count } } };
    return out;
}
static bool ae_json_has(const char *doc_text, int64_t doc_size, const char *path_text, int64_t path_size, size_t line, size_t column) {
    const ae_strview doc = { doc_text, doc_size };
    const ae_strview path = { path_text, path_size };

    int64_t vs = 0;
    int64_t ve = 0;
    int kind = 0;
    char why[192];
    return ae_json_locate(doc.ptr, doc.len, path.ptr, path.len, &vs, &ve, &kind, why, sizeof why, line, column) == AE_JSON_OK;
}

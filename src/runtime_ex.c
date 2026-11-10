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
#define AE_FILE_RESULT ae_e4
#define AE_FILE_OK AE_E4_0
#define AE_FILE_ERR AE_E4_1
#define AE_STATUS_RESULT ae_e5
#define AE_STATUS_OK AE_E5_0
#define AE_STATUS_ERR AE_E5_1

static AE_STATUS_RESULT ae_status_ok(void) {
    AE_STATUS_RESULT out = { AE_STATUS_OK, { { 0 } } };
    return out;
}
static AE_STATUS_RESULT ae_status_failed(ae_string message) {
    AE_STATUS_RESULT out = { AE_STATUS_ERR, { .v1 = { message } } };
    return out;
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

// ---- Path algebra -----------------------------------------------------------
// Separators: both '/' and '\' are accepted on input, but '/'-joined output is
// what this layer emits, because Windows accepts '/' and identical output on
// both platforms keeps tests and recorded results comparable.

static bool ae_is_separator(char c) { return c == '/' || c == '\\'; }

// Concatenate `dir` and `name`, inserting a separator only when needed.
static ae_string ae_path_concat(ae_strview dir, ae_strview name, bool separator, size_t line, size_t column) {
    const int64_t total = dir.len + name.len + (separator ? 1 : 0);
    char *buffer = malloc((size_t)total + 1);
    if (!buffer) ae_fail("out of memory", line, column);
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
        free(wide);
        AE_FILE_RESULT out = { AE_FILE_ERR, { .v1 = { ae_status_message("cannot read file metadata", status, line, column) } } };
        return out;
    }
    free(wide);
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
        free(source); free(target);
        return ae_status_failed(ae_str_from("out of memory", line, column));
    }
    const bool moved = MoveFileExW(source, target, MOVEFILE_REPLACE_EXISTING | MOVEFILE_COPY_ALLOWED) != 0;
    const unsigned long status = (unsigned long)GetLastError();
    free(source); free(target);
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
    free(wide);
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
    if (CreateDirectoryW(wide, NULL)) { free(wide); return none; }
    const unsigned long status = (unsigned long)GetLastError();
    free(wide);
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
    char *buffer = malloc((size_t)length + 1);
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
    free(buffer);
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
    free(wide);
    if (!removed) return ae_status_failed(ae_status_message("cannot remove directory", status, line, column));
    return ae_status_ok();
#else
    errno = 0;
    if (rmdir(path) != 0) return ae_status_failed(ae_io_message(errno, line, column));
    return ae_status_ok();
#endif
}

#include <stdio.h>
#include <string.h>
#include <zlib.h>
#include "kv.h"

/* Writes every entry as "key\tvalue\n" into a gzip file, so a restart can load it again. */
int kv_snapshot(const kv_table *t, const char *path)
{
    gzFile f = gzopen(path, "wb6");
    if (!f) {
        return -1;
    }
    for (size_t i = 0; i < t->capacity; i++) {
        if (t->slots[i].key) {
            gzprintf(f, "%s\t%s\n", t->slots[i].key, t->slots[i].value);
        }
    }
    return gzclose(f) == Z_OK ? 0 : -1;
}

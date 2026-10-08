#include "kv.h"

/* FNV-1a, 64 bits: quick, and spreads short keys well enough for an open-addressing table. */
uint64_t kv_hash(const char *key)
{
    uint64_t h = 14695981039346656037ULL;
    for (const unsigned char *p = (const unsigned char *)key; *p; p++) {
        h ^= *p;
        h *= 1099511628211ULL;
    }
    return h;
}

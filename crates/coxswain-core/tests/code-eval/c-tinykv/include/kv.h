/* tinykv: the table, the hash and the server, as the parts see each other. */
#ifndef KV_H
#define KV_H

#include <stddef.h>
#include <stdint.h>

#define KV_PORT 7379

typedef struct kv_entry {
    char *key;
    char *value;
    uint64_t hash;
} kv_entry;

typedef struct kv_table {
    kv_entry *slots;
    size_t capacity;
    size_t count;
} kv_table;

uint64_t kv_hash(const char *key);
kv_table *kv_table_new(size_t capacity);
int kv_set(kv_table *t, const char *key, const char *value);
const char *kv_get(const kv_table *t, const char *key);
int kv_del(kv_table *t, const char *key);
int kv_serve(kv_table *t, int port);
int kv_snapshot(const kv_table *t, const char *path);

#endif

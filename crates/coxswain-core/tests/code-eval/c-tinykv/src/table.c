#include <stdlib.h>
#include <string.h>
#include "kv.h"

/* The table grows to twice its size once it is three quarters full. */
#define MAX_LOAD 0.75

kv_table *kv_table_new(size_t capacity)
{
    kv_table *t = calloc(1, sizeof *t);
    t->slots = calloc(capacity, sizeof *t->slots);
    t->capacity = capacity;
    return t;
}

/* Linear probing: the first slot from the key's hash that is empty or holds the key. */
static kv_entry *find_slot(kv_entry *slots, size_t capacity, const char *key, uint64_t h)
{
    size_t i = h % capacity;
    while (slots[i].key && strcmp(slots[i].key, key) != 0) {
        i = (i + 1) % capacity;
    }
    return &slots[i];
}

static void grow(kv_table *t)
{
    size_t capacity = t->capacity * 2;
    kv_entry *slots = calloc(capacity, sizeof *slots);
    for (size_t i = 0; i < t->capacity; i++) {
        if (t->slots[i].key) {
            *find_slot(slots, capacity, t->slots[i].key, t->slots[i].hash) = t->slots[i];
        }
    }
    free(t->slots);
    t->slots = slots;
    t->capacity = capacity;
}

int kv_set(kv_table *t, const char *key, const char *value)
{
    if ((double)(t->count + 1) / t->capacity > MAX_LOAD) {
        grow(t);
    }
    uint64_t h = kv_hash(key);
    kv_entry *e = find_slot(t->slots, t->capacity, key, h);
    if (!e->key) {
        e->key = strdup(key);
        e->hash = h;
        t->count++;
    } else {
        free(e->value);
    }
    e->value = strdup(value);
    return 0;
}

const char *kv_get(const kv_table *t, const char *key)
{
    kv_entry *e = find_slot(t->slots, t->capacity, key, kv_hash(key));
    return e->key ? e->value : NULL;
}

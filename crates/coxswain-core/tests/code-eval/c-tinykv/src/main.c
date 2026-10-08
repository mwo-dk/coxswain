#include <pthread.h>
#include <stdio.h>
#include <unistd.h>
#include "kv.h"

/* Every 60 seconds, a snapshot to tinykv.gz, on a thread of its own. */
static void *snapshots(void *arg)
{
    for (;;) {
        sleep(60);
        kv_snapshot(arg, "tinykv.gz");
    }
    return NULL;
}

int main(void)
{
    kv_table *t = kv_table_new(1024);
    pthread_t th;
    pthread_create(&th, NULL, snapshots, t);
    printf("tinykv on port %d\n", KV_PORT);
    return kv_serve(t, KV_PORT);
}

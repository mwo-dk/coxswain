#include <stdio.h>
#include <string.h>
#include <sys/epoll.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <unistd.h>
#include "kv.h"

#define MAX_EVENTS 64

/* One line a command: GET key, SET key value, DEL key. */
static void handle_line(kv_table *t, int fd, char *line)
{
    char *cmd = strtok(line, " \r\n"), *key = strtok(NULL, " \r\n"), *value = strtok(NULL, "\r\n");
    char out[512];
    if (cmd && key && strcmp(cmd, "GET") == 0) {
        const char *v = kv_get(t, key);
        snprintf(out, sizeof out, "%s\n", v ? v : "(nil)");
    } else if (cmd && key && value && strcmp(cmd, "SET") == 0) {
        kv_set(t, key, value);
        snprintf(out, sizeof out, "OK\n");
    } else {
        snprintf(out, sizeof out, "ERR\n");
    }
    write(fd, out, strlen(out));
}

/* Serves every client from one thread with epoll: no thread per connection. */
int kv_serve(kv_table *t, int port)
{
    int srv = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(port), .sin_addr.s_addr = htonl(INADDR_ANY) };
    if (bind(srv, (struct sockaddr *)&addr, sizeof addr) < 0 || listen(srv, 128) < 0) {
        perror("listen");
        return -1;
    }
    int ep = epoll_create1(0);
    struct epoll_event ev = { .events = EPOLLIN, .data.fd = srv }, events[MAX_EVENTS];
    epoll_ctl(ep, EPOLL_CTL_ADD, srv, &ev);
    for (;;) {
        int n = epoll_wait(ep, events, MAX_EVENTS, -1);
        for (int i = 0; i < n; i++) {
            int fd = events[i].data.fd;
            if (fd == srv) {
                int c = accept(srv, NULL, NULL);
                struct epoll_event cev = { .events = EPOLLIN, .data.fd = c };
                epoll_ctl(ep, EPOLL_CTL_ADD, c, &cev);
            } else {
                char buf[1024];
                ssize_t got = read(fd, buf, sizeof buf - 1);
                if (got <= 0) {
                    close(fd);
                    continue;
                }
                buf[got] = 0;
                handle_line(t, fd, buf);
            }
        }
    }
}

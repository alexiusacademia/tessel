#include <stdio.h>
#include <stdlib.h>
#include <string.h>
int main(void) {
    int n = 1000000;
    char **parts = malloc(sizeof(char *) * n);
    size_t total = 0;
    for (int i = 0; i < n; i++) {
        char buf[32];
        int len = snprintf(buf, sizeof buf, "item%d", i);
        parts[i] = malloc(len + 1);
        memcpy(parts[i], buf, len + 1);
        total += len + 1;
    }
    char *text = malloc(total);
    size_t pos = 0;
    for (int i = 0; i < n; i++) {
        size_t len = strlen(parts[i]);
        memcpy(text + pos, parts[i], len);
        pos += len;
        if (i + 1 < n) text[pos++] = ',';
        free(parts[i]);
    }
    text[pos] = 0;
    long count = 0;
    char *start = text;
    for (char *p = text;; p++) {
        if (*p == ',' || *p == 0) {
            if (p > start && p[-1] == '7') count++;
            if (*p == 0) break;
            start = p + 1;
        }
    }
    printf("%zu %ld\n", pos, count);
    free(parts); free(text);
    return 0;
}

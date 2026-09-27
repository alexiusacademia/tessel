#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
static int cmp(const void *a, const void *b) { int64_t x = *(const int64_t *)a, y = *(const int64_t *)b; return (x > y) - (x < y); }
int main(void) {
    int n = 1000000; int64_t x = 42;
    int64_t *a = malloc(sizeof(int64_t) * n);
    for (int i = 0; i < n; i++) { x = (x * 1103515245 + 12345) % 2147483648; a[i] = x; }
    qsort(a, n, sizeof(int64_t), cmp);
    printf("%lld %lld %lld\n", (long long)a[0], (long long)a[500000], (long long)a[999999]);
    free(a);
    return 0;
}

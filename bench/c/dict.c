#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>
typedef struct { int64_t key, value; bool used; } Slot;
static Slot *slots; static size_t cap, count;
static size_t hash(int64_t k) { uint64_t x = (uint64_t)k * 0x9E3779B97F4A7C15ull; return (size_t)(x ^ (x >> 32)); }
static void put(int64_t k, int64_t v);
static void grow(void) {
    Slot *old = slots; size_t oldcap = cap;
    cap = cap ? cap * 2 : 16; slots = calloc(cap, sizeof(Slot)); count = 0;
    for (size_t i = 0; i < oldcap; i++) if (old[i].used) put(old[i].key, old[i].value);
    free(old);
}
static void put(int64_t k, int64_t v) {
    if ((count + 1) * 4 > cap * 3) grow();
    size_t i = hash(k) & (cap - 1);
    while (slots[i].used && slots[i].key != k) i = (i + 1) & (cap - 1);
    if (!slots[i].used) { slots[i].used = true; slots[i].key = k; count++; }
    slots[i].value = v;
}
static bool get(int64_t k, int64_t *v) {
    size_t i = hash(k) & (cap - 1);
    while (slots[i].used) { if (slots[i].key == k) { *v = slots[i].value; return true; } i = (i + 1) & (cap - 1); }
    return false;
}
int main(void) {
    for (int64_t i = 0; i < 1000000; i++) put(i * 7, i);
    int64_t sum = 0, v;
    for (int64_t k = 0; k < 2000000; k++) if (get(k, &v)) sum += v;
    printf("%zu %lld\n", count, (long long)sum);
    return 0;
}

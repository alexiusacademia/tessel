#include <stdio.h>
#include <stdlib.h>
#include <stdbool.h>
int main(void) {
    long limit = 20000000;
    bool *sieve = malloc(limit);
    for (long i = 0; i < limit; i++) sieve[i] = true;
    long count = 0;
    for (long i = 2; i < limit; i++) {
        if (sieve[i]) {
            count++;
            for (long j = i * i; j < limit; j += i) sieve[j] = false;
        }
    }
    printf("%ld\n", count);
    free(sieve);
    return 0;
}

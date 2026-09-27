#include <stdio.h>
int main(void) {
    int size = 1200; long inside = 0;
    for (int py = 0; py < size; py++)
        for (int px = 0; px < size; px++) {
            double cx = (double)px / size * 3.0 - 2.0, cy = (double)py / size * 3.0 - 1.5;
            double x = 0, y = 0; int n = 0;
            while (n < 100 && x * x + y * y <= 4.0) { double t = x * x - y * y + cx; y = 2.0 * x * y + cy; x = t; n++; }
            if (n == 100) inside++;
        }
    printf("%ld\n", inside);
    return 0;
}

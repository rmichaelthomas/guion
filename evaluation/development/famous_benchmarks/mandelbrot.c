#include <stdio.h>

static int escape_steps(double cx, double cy, int limit) {
    double x = 0.0;
    double y = 0.0;
    int steps = 0;
    while (steps < limit && x * x + y * y <= 4.0) {
        const double next_x = x * x - y * y + cx;
        y = 2.0 * x * y + cy;
        x = next_x;
        steps += 1;
    }
    return steps;
}

int main(void) {
    const int width = 96;
    const int height = 64;
    const int limit = 80;
    const double step = 0.03125;
    int total_steps = 0;
    int interior = 0;
    double cy = -1.0;
    for (int y_index = 0; y_index < height; ++y_index, cy += step) {
        double cx = -2.0;
        for (int x_index = 0; x_index < width; ++x_index, cx += step) {
            const int steps = escape_steps(cx, cy, limit);
            total_steps += steps;
            interior += steps == limit;
        }
    }
    printf("%d\n%d\n", total_steps, interior);
    return 0;
}

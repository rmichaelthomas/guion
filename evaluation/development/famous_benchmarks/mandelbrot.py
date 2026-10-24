#!/usr/bin/env python3
"""Small, dependency-free Mandelbrot kernel for Veritas/Python comparison."""

WIDTH = 96
HEIGHT = 64
LIMIT = 80
STEP = 0.03125


def escape_steps(cx: float, cy: float, limit: int) -> int:
    x = 0.0
    y = 0.0
    steps = 0
    while steps < limit and x * x + y * y <= 4.0:
        next_x = x * x - y * y + cx
        y = 2.0 * x * y + cy
        x = next_x
        steps += 1
    return steps


def main() -> None:
    y_index = 0
    cy = -1.0
    total_steps = 0
    interior = 0
    while y_index < HEIGHT:
        x_index = 0
        cx = -2.0
        while x_index < WIDTH:
            steps = escape_steps(cx, cy, LIMIT)
            total_steps += steps
            if steps == LIMIT:
                interior += 1
            cx += STEP
            x_index += 1
        cy += STEP
        y_index += 1
    print(total_steps)
    print(interior)


if __name__ == "__main__":
    main()

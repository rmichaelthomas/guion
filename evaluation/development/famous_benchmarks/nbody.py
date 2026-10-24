#!/usr/bin/env python3
"""Reference implementation of the classic five-body N-body workload."""

from __future__ import annotations

import math

PI = 3.141592653589793
SOLAR_MASS = 4.0 * PI * PI
DAYS_PER_YEAR = 365.24

# (x, y, z, vx, vy, vz, mass), using the standard solar-system benchmark data.
BODIES = [
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, SOLAR_MASS],
    [4.841431442464721, -1.1603200440274284, -0.10362204447112311,
     0.001660076642744037 * DAYS_PER_YEAR, 0.007699011184197404 * DAYS_PER_YEAR,
     -0.0000690460016972063 * DAYS_PER_YEAR, 0.0009547919384243266 * SOLAR_MASS],
    [8.34336671824458, 4.124798564124305, -0.4035234171143214,
     -0.002767425107268624 * DAYS_PER_YEAR, 0.004998528012349172 * DAYS_PER_YEAR,
     0.00002304172975737639 * DAYS_PER_YEAR, 0.0002858859806661308 * SOLAR_MASS],
    [12.894369562139131, -15.111151401698631, -0.22330757889265573,
     0.002964601375647616 * DAYS_PER_YEAR, 0.0023784717395948095 * DAYS_PER_YEAR,
     -0.00002965895685402376 * DAYS_PER_YEAR, 0.00004366244043351563 * SOLAR_MASS],
    [15.379697114850917, -25.919314609987964, 0.17925877295037118,
     0.0026806777249038932 * DAYS_PER_YEAR, 0.001628241700382423 * DAYS_PER_YEAR,
     -0.00009515922545197159 * DAYS_PER_YEAR, 0.000051513890204661145 * SOLAR_MASS],
]


def offset_momentum() -> None:
    for axis in range(3):
        momentum = sum(body[axis + 3] * body[6] for body in BODIES[1:])
        BODIES[0][axis + 3] = -momentum / SOLAR_MASS


def energy() -> float:
    result = 0.0
    for i, body in enumerate(BODIES):
        result += 0.5 * body[6] * sum(body[axis + 3] ** 2 for axis in range(3))
        for other in BODIES[i + 1:]:
            distance = math.sqrt(sum((body[axis] - other[axis]) ** 2 for axis in range(3)))
            result -= body[6] * other[6] / distance
    return result


def advance(dt: float) -> None:
    for i, body in enumerate(BODIES):
        for other in BODIES[i + 1:]:
            delta = [body[axis] - other[axis] for axis in range(3)]
            distance_squared = sum(value * value for value in delta)
            distance = math.sqrt(distance_squared)
            magnitude = dt / (distance_squared * distance)
            for axis in range(3):
                body[axis + 3] -= delta[axis] * other[6] * magnitude
                other[axis + 3] += delta[axis] * body[6] * magnitude
        for axis in range(3):
            body[axis] += dt * body[axis + 3]


def main() -> None:
    offset_momentum()
    print(energy())
    for _ in range(20000):
        advance(0.01)
    print(energy())


if __name__ == "__main__":
    main()

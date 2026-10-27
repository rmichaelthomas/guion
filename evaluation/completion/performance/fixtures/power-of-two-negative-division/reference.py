COUNT = 25_000


def truncating_divide(value: int, divisor: int) -> int:
    magnitude = abs(value) // abs(divisor)
    return -magnitude if (value < 0) != (divisor < 0) else magnitude


def calculate(count: int) -> int:
    value = -count // 2
    total = 0
    for _ in range(count):
        total += value // -4
        total += value // -8
        total += value // -16
        total += value % -4
        total += value % -8
        total += value % -16
        total += truncating_divide(value, -2)
        total += truncating_divide(value, -4)
        total += truncating_divide(value, -8)
        value += 1
    return total


def main() -> None:
    print(calculate(COUNT))


if __name__ == "__main__":
    main()

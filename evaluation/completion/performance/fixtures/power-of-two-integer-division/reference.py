COUNT = 25_000


def truncating_divide(value: int, divisor: int) -> int:
    if value < 0:
        return -((-value) // divisor)
    return value // divisor


def calculate(count: int) -> int:
    value = -count
    total = 0
    for _ in range(count):
        total += truncating_divide(value, 1)
        total += truncating_divide(value, 2)
        total += truncating_divide(value, 4)
        total += truncating_divide(value, 8)
        total += truncating_divide(value, 16)
        value += 1
    return total


def main() -> None:
    print(calculate(COUNT))


if __name__ == "__main__":
    main()

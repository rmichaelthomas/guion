COUNT = 25_000


def calculate(count: int) -> int:
    value = -count
    total = 0
    for _ in range(count):
        total += value // 1 + value % 1
        total += value // 4 + value % 4
        total += value // 8 + value % 8
        total += value // 16 + value % 16
        value += 1
    return total


def main() -> None:
    print(calculate(COUNT))


if __name__ == "__main__":
    main()

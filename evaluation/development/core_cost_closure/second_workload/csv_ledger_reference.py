import csv
import sys


def main() -> int:
    if len(sys.argv) != 3 or sys.argv[1] != "--input":
        return 2
    with open(sys.argv[2], newline="", encoding="utf-8") as handle:
        reader = csv.reader(handle)
        next(reader)
        counts: dict[str, int] = {}
        errors: dict[str, int] = {}
        totals: dict[str, int] = {}
        peaks: dict[str, int] = {}
        invalid = 0
        for fields in reader:
            try:
                status = int(fields[1])
                latency = int(fields[2])
            except (ValueError, IndexError):
                invalid += 1
                continue
            if len(fields) != 4 or fields[0] == "":
                invalid += 1
                continue
            service = fields[0]
            if service not in counts:
                counts[service] = 0
                errors[service] = 0
                totals[service] = 0
                peaks[service] = 0
            counts[service] += 1
            if status >= 500:
                errors[service] += 1
            totals[service] += latency
            if latency > peaks[service]:
                peaks[service] = latency
    for service in counts:
        print(f"{service} {counts[service]} {errors[service]} {totals[service]} {peaks[service]}")
    print(f"invalid {invalid}")
    return 0


raise SystemExit(main())

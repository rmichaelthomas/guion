#!/usr/bin/env python3
"""Independent standard-library baseline for the reconciliation workload."""

import json
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 4:
        print("Usage: python3 reference.py INVENTORY.json ADJUSTMENTS.json OUTPUT.json")
        return 2
    inventory_path, adjustments_path, output_path = map(Path, sys.argv[1:])
    try:
        inventory = json.loads(inventory_path.read_text(encoding="utf-8"))
        adjustments_document = json.loads(adjustments_path.read_text(encoding="utf-8"))
    except OSError as error:
        print(f"Cannot read input: {error.__class__.__name__}")
        return 1
    except json.JSONDecodeError:
        print("Invalid input JSON: JSON_PARSE")
        return 1

    quantities: dict[str, int] = {}
    order: list[str] = []
    for row in inventory["items"]:
        sku, quantity = row["sku"], row["quantity"]
        if sku in quantities:
            print(f"Duplicate inventory SKU: {sku}")
            return 2
        if quantity < 0:
            print(f"Inventory quantity cannot be negative: {sku}")
            return 2
        quantities[sku] = quantity
        order.append(sku)

    adjustments = adjustments_document["adjustments"]
    changes = []
    for row in adjustments:
        sku, delta, reason, source = row["sku"], row["delta"], row["reason"], row["source"]
        if sku not in quantities:
            quantities[sku] = 0
            order.append(sku)
        after = quantities[sku] + delta
        if after < 0:
            print(f"Adjustment would make inventory negative: {sku}")
            return 1
        quantities[sku] = after
        changes.append({
            "sku": sku,
            "delta": delta,
            "reason": reason,
            "source": source,
            "balance": after,
        })

    total = sum(quantities[sku] for sku in order)
    output_path.write_text(
        json.dumps(
            {"items": quantities, "changes": changes},
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        ),
        encoding="utf-8",
    )
    print("Adjustments:")
    print(len(adjustments))
    print("Products:")
    print(len(order))
    print("Units:")
    print(total)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

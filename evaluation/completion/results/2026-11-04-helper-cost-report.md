# Passing lists, maps and text through helper functions (2026-11-04)

## What was wrong

A function that took a list, map or text and returned it updated copied the whole value on every call, and returning a list or map re-checked every item in it. A loop that calls such a helper therefore cost the square of its size. The same loop written inline did not.

Three causes, all fixed in the same change:

- **The return walked the whole value.** Items are limit-checked when they are inserted, so the return now checks a list or map by its size only.
- **The update copied.** A caller's variable still held the old value while the helper updated it, so the helper had to copy. The call now releases the caller's variable when the result is stored straight back into it (`set m = helper(m, ...)`) or returned.
- **Machine code could not see ownership.** Machine code passes lists and maps as shared handles, so a function that hands one through a call, or builds text that way, now runs in the interpreter, where ownership is exact.

`list_set`, lists of floats and text appends use the same rule as `list_push` and `map_set`.

## Measurements

Run with `python3 evaluation/completion/results/2026-11-04-helper-cost-benchmarks/run.py --binary <guion>`. Each program passes a collection or text through a helper and stores the result back into the same variable. The machine is an Apple Silicon Mac. "Before" is main before this change (`d42907e`) (one run per program, because each is slow); "after" is this change (median of five runs).

| Program | Before | After |
|---|---|---|
| `map_helper.gu`: 100,000 updates, 10,000 keys | 10.941 s | 0.043 s |
| `list_helper.gu`: 40,000 appends | 9.212 s | 0.012 s |
| `list_set_helper.gu`: 40,000 appends and sets | 9.265 s | 0.012 s |
| `text_helper.gu`: 160,000 appends | 1.166 s | 0.032 s |

Earlier, on the same machine and build as "before", a list helper at 160,000 appends took about 159 s.

The nine frozen applications and the 16,384-package lockfile case are unchanged within run-to-run noise. A baseline rebuilt from main before this change and timed back to back against this change differed less than the baseline's own two runs differed from each other (release-notes, sla-report, jsonl-summary, nine trials each).

## Limits that remain

- **A result stored in a different variable still copies.** `let t = add(s, x)` leaves `s` in use, so the helper must copy. This is correct and costs time in proportion to the size of the value. The fast forms are `set s = helper(s, ...)` and returning the helper's result directly.
- **Functions that hand a list or map through a call run in the interpreter, not machine code.** Loops that do that are no longer slow, but they do not get the machine-code speed-up.
- **Plain text functions are unchanged.** A function like `concatenate(left, right)` that is not called as `set s = f(s, ...)` stays in machine code.
- **Records that hold lists or text** do not yet run in machine code. That is separate work.

## Tests

- `collections_updated_through_helpers_keep_every_alias_intact` and `list_set_and_float_lists_through_helpers_keep_aliases_and_stay_linear`: a caller that keeps the old value, passes one value twice, or reuses the value after the call still sees the old value, on the interpreter and on machine code.
- `collections_handed_through_helpers_stay_linear_under_the_allocation_budget` and `text_built_through_a_helper_is_appended_in_place_and_aliases_are_kept`: 20,000 to 60,000 iterations inside a 64 MB allocation budget, which the old behaviour exhausted. The list test above also loops 20,000 times inside the same budget.

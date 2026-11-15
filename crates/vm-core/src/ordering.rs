//! Sorting helpers shared by the native runtime and the portable (browser) host, so both order
//! values and charge the budget identically.

use crate::{charge_allocation, cloned_value_allocation, step_limit_error, Budget};
use guion_model::{BigInt, RuntimeError, SharedText, Span, Value};
use std::{cmp::Ordering as CmpOrdering, mem::size_of};

pub fn consume_sort_step(budget: &Budget, span: Span) -> Result<(), RuntimeError> {
    if budget.is_cancelled() {
        return Err(RuntimeError::new(
            "R_CANCELLED",
            "parallel task was cancelled after a sibling failed",
            span,
        ));
    }
    if !budget.consume() {
        return Err(step_limit_error(budget, span));
    }
    Ok(())
}

/// A sort key as the sorts compare it: each list holds one kind.
pub enum SortKey {
    Int(BigInt),
    Float(f64),
    Text(SharedText),
}

impl SortKey {
    pub fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Int(key) => Some(SortKey::Int(key.clone())),
            Value::Float64(key) => Some(SortKey::Float(key.get())),
            Value::Text(key) => Some(SortKey::Text(key.clone())),
            _ => None,
        }
    }

    /// Keys of different kinds never meet in a verified artifact; equal keeps the sort total.
    pub fn compare(&self, other: &SortKey) -> CmpOrdering {
        match (self, other) {
            (SortKey::Int(left), SortKey::Int(right)) => left.cmp(right),
            (SortKey::Float(left), SortKey::Float(right)) => {
                guion_model::sort_order_f64(*left, *right)
            }
            // Unicode scalar order: the UTF-8 bytes of valid text sort exactly like the scalars,
            // as sorted map keys do.
            (SortKey::Text(left), SortKey::Text(right)) => left.as_bytes().cmp(right.as_bytes()),
            _ => CmpOrdering::Equal,
        }
    }
}

/// Bytes charged to the allocation budget for sorting `values`: the cloned list plus the
/// order and visited bookkeeping, plus `key_bytes` per item for the keys held beside it.
pub fn sort_allocation(values: &[Value], key_bytes: usize) -> usize {
    let length = values.len();
    let value_allocation = size_of::<Vec<Value>>()
        .saturating_add(length.saturating_mul(size_of::<Value>()))
        .saturating_add(
            values
                .iter()
                .map(cloned_value_allocation)
                .fold(0usize, usize::saturating_add),
        );
    let sort_storage = size_of::<Vec<usize>>()
        .saturating_mul(2)
        .saturating_add(size_of::<Vec<bool>>())
        .saturating_add(
            length.saturating_mul(
                key_bytes
                    .saturating_add(size_of::<usize>().saturating_mul(2))
                    .saturating_add(size_of::<bool>()),
            ),
        )
        .saturating_add(size_of::<Vec<BigInt>>());
    value_allocation.saturating_add(sort_storage)
}

/// The stable order of `0..length` under `compare(a, b)` on item indexes. Insertion sort up to
/// 32 items, bottom-up merge sort beyond; every comparison costs one instruction of the guest
/// budget, exactly as `sort_by_int_key` (format 41) charges. Equal items keep input order.
pub fn stable_sort_order<F>(
    length: usize,
    mut compare: F,
    budget: &Budget,
    span: Span,
) -> Result<Vec<usize>, RuntimeError>
where
    F: FnMut(usize, usize) -> CmpOrdering,
{
    let mut order: Vec<usize> = (0..length).collect();
    if length <= 32 {
        for index in 1..length {
            let value = order[index];
            let mut position = index;
            while position > 0 {
                consume_sort_step(budget, span)?;
                if compare(value, order[position - 1]) == CmpOrdering::Less {
                    order[position] = order[position - 1];
                    position -= 1;
                } else {
                    break;
                }
            }
            order[position] = value;
        }
    } else {
        let mut merged = Vec::with_capacity(length);
        let mut width = 1usize;
        while width < length {
            merged.clear();
            let mut run_start = 0usize;
            while run_start < length {
                let middle = run_start.saturating_add(width).min(length);
                let run_end = middle.saturating_add(width).min(length);
                let (mut left, mut right) = (run_start, middle);
                while left < middle && right < run_end {
                    consume_sort_step(budget, span)?;
                    if compare(order[left], order[right]) != CmpOrdering::Greater {
                        merged.push(order[left]);
                        left += 1;
                    } else {
                        merged.push(order[right]);
                        right += 1;
                    }
                }
                merged.extend_from_slice(&order[left..middle]);
                merged.extend_from_slice(&order[right..run_end]);
                run_start = run_end;
            }
            std::mem::swap(&mut order, &mut merged);
            width = width.saturating_mul(2);
        }
    }
    Ok(order)
}

/// Rearranges `values` in place so that position `i` holds what was at `order[i]`.
pub fn apply_sort_order<T>(values: &mut [T], order: &[usize]) {
    let mut visited = vec![false; values.len()];
    for start in 0..values.len() {
        if visited[start] {
            continue;
        }
        let mut current = start;
        loop {
            visited[current] = true;
            let next = order[current];
            if next == start {
                break;
            }
            values.swap(current, next);
            current = next;
        }
    }
}

/// Charges `bytes` for a list copy, as the sorts and higher-order builtins do before running.
pub fn charge_list_copy(budget: &Budget, items: &[Value], span: Span) -> Result<(), RuntimeError> {
    charge_allocation(
        budget,
        size_of::<Vec<Value>>().saturating_add(items.len().saturating_mul(size_of::<Value>())),
        span,
    )
}

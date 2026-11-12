use guion_model::{
    Artifact, BigInt, FileErrorValue, Float64, HttpErrorValue, JsonErrorValue, NativeResourceValue,
    Op, RuntimeError, ShareAcrossThreads, Shared, SharedJson, SharedText, Span, TaskErrorValue,
    TextMap, Type, Value,
};
pub use guion_vm_core::CancellationToken;
use guion_vm_core::{
    build_register_programs, charge_allocation, check_call_depth, cloned_value_allocation,
    encoded_json_string_len, finish_json_value, finish_shared_json, json_exact_integer,
    map_clone_allocation, new_vm_frame, opcode_profile_name, shared_json_push_local_allocation,
    shared_json_set_local_allocation, value_matches, verify, Budget, FrameStart, HostOpContext,
    OutputStream, RegisterPrograms, RuntimeProfileCounters, VmFrame, VmHost, VmLimits,
};
pub use guion_vm_core::{live_memory, set_live_memory_probe};
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::Write as IoWrite,
    mem::size_of,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
mod fs_ops;
mod json_projection;
mod native_backend;
mod net_ops;
mod process_ops;
#[cfg(feature = "python-interop")]
pub mod python_bridge;
pub mod stdlib_data;
pub mod stdlib_services;
use native_backend::{
    decode_inline_native_record, encode_inline_native_record, native_inline_record,
    native_record_has_native_layout, native_record_words, NativeBackend, NativeCode,
    NativeFloatList, NativeHandleList, NativeIntList, NativeRecord, NativeSymbols,
    NativeTaggedValue, NATIVE_CONSTANT_HANDLE_BASE, NATIVE_INLINE_LIST_ITEMS,
    NATIVE_TAGGED_OPTION_NONE, NATIVE_TAGGED_OPTION_SOME, NATIVE_TAGGED_RESULT_ERR,
    NATIVE_TAGGED_RESULT_OK,
};

struct JsonTextPairRows<'a> {
    groups: &'a TextMap,
    key_field: &'a str,
    value_field: &'a str,
    row_count: usize,
}

impl serde::Serialize for JsonTextPairRows<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;

        let mut sequence = serializer.serialize_seq(Some(self.row_count))?;
        for (key, values) in self.groups {
            let Value::List(values) = values else {
                return Err(serde::ser::Error::custom(
                    "json_stringify_pairs requires Map[Text, List[Text]]",
                ));
            };
            for value in values.iter() {
                let Value::Text(value) = value else {
                    return Err(serde::ser::Error::custom(
                        "json_stringify_pairs requires Map[Text, List[Text]]",
                    ));
                };
                sequence.serialize_element(&JsonTextPair {
                    key_field: self.key_field,
                    key,
                    value_field: self.value_field,
                    value: value.as_ref(),
                })?;
            }
        }
        sequence.end()
    }
}

struct JsonTextPair<'a> {
    key_field: &'a str,
    key: &'a str,
    value_field: &'a str,
    value: &'a str,
}

impl serde::Serialize for JsonTextPair<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut object = serializer.serialize_map(Some(2))?;
        object.serialize_entry(self.key_field, self.key)?;
        object.serialize_entry(self.value_field, self.value)?;
        object.end()
    }
}

#[cfg(test)]
mod native_inline_arena_tests {
    use super::{
        Budget, CancellationToken, NativeIntList, NativeListArena, NativeTaggedValue, Value,
        NATIVE_TAGGED_OPTION_SOME, NATIVE_TAGGED_RESULT_OK,
    };

    #[test]
    fn inline_list_addresses_and_values_survive_arena_growth() {
        let mut arena = NativeListArena::new(std::ptr::null(), 10000, 10000, 0, None);
        let first = arena
            .allocate_inline_int(&[10, 20, 30], 1, 99)
            .cast::<NativeIntList>();
        for value in 0..1000 {
            let current = arena
                .allocate_inline_int(&[value, value + 1], 0, value + 2)
                .cast::<NativeIntList>();
            let descriptor = unsafe { &*current };
            assert_eq!(unsafe { *descriptor.data }, value + 2);
        }
        let descriptor = unsafe { &*first };
        assert_eq!(
            unsafe { std::slice::from_raw_parts(descriptor.data, descriptor.len) },
            &[10, 99, 30]
        );
    }

    #[test]
    fn tagged_value_arena_uses_inline_storage_and_stable_overflow_growth() {
        let mut arena = NativeListArena::new(std::ptr::null(), 10000, 10000, 0, None);
        let value = || NativeTaggedValue {
            kind: 1,
            payload_tag: 1,
            payload: 7,
            clone_charge_bytes: 0,
        };

        let first_handle = arena.store_tagged_descriptor(value()).unwrap();
        for _ in 1..8 {
            arena.store_tagged_descriptor(value()).unwrap();
        }
        assert!(arena.tagged_values.is_empty());
        let first_descriptor =
            arena.tagged_value(first_handle).unwrap() as *const NativeTaggedValue;
        arena.store_tagged_descriptor(value()).unwrap();
        assert_eq!(arena.tagged_values.len(), 1);
        assert_eq!(arena.tagged_values[0].capacity(), 256);
        assert_eq!(
            arena.tagged_value(first_handle).unwrap() as *const NativeTaggedValue,
            first_descriptor
        );
    }

    #[test]
    fn tagged_value_creation_defers_recursive_clone_charge_estimation() {
        let budget = Budget::new(1000, 10000, CancellationToken::new(), None);
        let mut arena = NativeListArena::new(&budget, 10000, 10000, 0, None);
        let payload = Value::Record(
            "EventLine".into(),
            guion_model::Shared::new(vec![Value::Text("record payload".into())]),
        );
        let payload_handle = arena.store_value(payload.clone()).unwrap();
        assert!(arena.native_value_handle_exists(payload_handle));
        let handle = arena
            .store_native_tagged_value(NATIVE_TAGGED_RESULT_OK, 4, payload_handle)
            .unwrap();

        let expected = std::mem::size_of::<Value>() + super::cloned_value_allocation(&payload);
        assert_eq!(arena.tagged_value(handle).unwrap().clone_charge_bytes, 0);
        assert_eq!(arena.tagged_clone_charge_bytes(handle), Some(expected));
        assert_eq!(
            arena.tagged_value(handle).unwrap().clone_charge_bytes,
            expected as u64
        );
    }

    #[test]
    fn native_scalar_tagged_values_cache_exact_clone_charge_at_creation() {
        let budget = Budget::new(1000, 10000, CancellationToken::new(), None);
        let mut arena = NativeListArena::new(&budget, 1000, 10000, 0, None);
        let expected = std::mem::size_of::<Value>() as u64;
        for (kind, payload_tag, payload) in [
            (NATIVE_TAGGED_RESULT_OK, 1, 42),
            (NATIVE_TAGGED_OPTION_SOME, 2, 1.5_f64.to_bits() as i64),
            (NATIVE_TAGGED_OPTION_SOME, 3, 1),
        ] {
            let handle = arena
                .store_native_tagged_value(kind, payload_tag, payload)
                .expect("native scalar tagged value is stored");
            assert_eq!(
                arena.tagged_value(handle).unwrap().clone_charge_bytes,
                expected
            );
            assert_eq!(
                arena.tagged_clone_charge_bytes(handle),
                Some(expected as usize)
            );
        }
    }
}

// JIT code keeps raw pointers to list descriptors and tagged values. Boxes and
// inline slots plus fixed-capacity overflow chunks keep their addresses stable.
#[allow(clippy::vec_box)]
/// Boxed descriptors with stable addresses and constant-time address membership.
struct AddressedBoxes<T> {
    items: Vec<Box<T>>,
    addresses: std::collections::HashSet<usize, std::hash::BuildHasherDefault<AddressHasher>>,
}

#[derive(Default)]
struct AddressHasher(u64);

impl std::hash::Hasher for AddressHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
        }
    }

    fn write_usize(&mut self, value: usize) {
        self.0 = (value as u64 >> 3).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

impl<T> AddressedBoxes<T> {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            addresses: Default::default(),
        }
    }

    fn try_reserve(&mut self, additional: usize) -> Result<(), std::collections::TryReserveError> {
        self.items.try_reserve(additional)?;
        self.addresses.try_reserve(additional)
    }

    fn push(&mut self, item: Box<T>) {
        self.addresses.insert((&*item as *const T) as usize);
        self.items.push(item);
    }

    fn last_mut(&mut self) -> Option<&mut Box<T>> {
        self.items.last_mut()
    }

    fn clear(&mut self) {
        self.items.clear();
        self.addresses.clear();
    }

    fn find_address(&self, address: usize) -> Option<&T> {
        if !self.addresses.contains(&address) {
            return None;
        }
        // The address is a live boxed descriptor owned by `items`.
        Some(unsafe { &*(address as *const T) })
    }
}

impl<T> std::ops::Deref for AddressedBoxes<T> {
    type Target = Vec<Box<T>>;

    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

struct NativeListArena {
    float_lists: AddressedBoxes<NativeFloatList>,
    float_storage: Vec<Vec<Float64>>,
    int_lists: AddressedBoxes<NativeIntList>,
    inline_int_lists: Vec<Vec<NativeIntList>>,
    int_storage: Vec<Vec<i64>>,
    handle_lists: AddressedBoxes<NativeHandleList>,
    handle_storage: Vec<Vec<i64>>,
    // Native code holds raw pointers to each descriptor, so each needs a stable address.
    #[allow(clippy::vec_box)]
    native_records: Vec<Box<NativeRecord>>,
    native_record_types: Vec<usize>,
    native_record_fields: Vec<Box<[i64]>>,
    native_record_indices: HashMap<i64, usize>,
    values: Vec<Value>,
    constant_values: Arc<Vec<Value>>,
    inline_tagged_values: [NativeTaggedValue; NATIVE_INLINE_TAGGED_VALUES],
    inline_tagged_value_len: usize,
    tagged_values: Vec<Vec<NativeTaggedValue>>,
    tagged_chunk_starts: std::collections::BTreeMap<usize, usize>,
    native_vm_continuations: Vec<NativeVmContinuationFrame>,
    runtime_context: Option<NativeRuntimeContext>,
    host_error: Option<RuntimeError>,
    budget: *const Budget,
    profile: Option<Arc<RuntimeProfileCounters>>,
    max_collection_items: usize,
    charged_bytes: usize,
    output_limit_bytes: usize,
    initial_output_bytes: usize,
    committed_output_bytes: usize,
    pending_output: String,
    pending_print_bytes: Vec<usize>,
    reclaim_floor: usize,
    reclamations: u64,
}

const NATIVE_INLINE_TAGGED_VALUES: usize = 8;
/// Arena entries that must accumulate before a loop safepoint attempts to retire dead handles.
const NATIVE_ARENA_RECLAIM_MIN_ENTRIES: usize = 2048;
/// List items an entry must copy into a fresh native frame before its cost is tracked.
const LIST_ITEMS_TRACKED_AT_ENTRY: usize = 4096;
/// Copy-dominated large entries after which VM callers run the function in the VM.
const COPY_DOMINATED_ENTRY_LIMIT: u8 = 8;
/// Safepoint root kind for a `List[Text]` local stored as a native handle list.
pub(crate) const NATIVE_SAFEPOINT_TEXT_LIST_TAG: i64 = 5;

pub(crate) struct NativeVmContinuationFrame {
    pub(crate) function_index: usize,
    pub(crate) ip: usize,
    pub(crate) depth: usize,
    pub(crate) call_site: Option<Span>,
    pub(crate) locals: Vec<Value>,
    pub(crate) stack: Vec<Value>,
}

#[derive(Clone)]
struct NativeRuntimeContext {
    instance: *mut Instance,
    artifact: Arc<Artifact>,
    budget: Arc<Budget>,
    limits: VmLimits,
}

#[derive(Clone, Copy)]
struct NativeOperationRequest<'a> {
    operation: i64,
    function_index: i64,
    instruction_index: i64,
    depth: i64,
    arguments: &'a [i64],
    argument_tags: &'a [i64],
    result_tag: i64,
}

#[derive(Clone, Copy)]
struct NativeJsonOperationRequest<'a> {
    operation: i64,
    arguments: &'a [i64],
    argument_tags: &'a [i64],
    result_tag: i64,
    span: Span,
    operation_name: &'static str,
    runtime: &'a NativeRuntimeContext,
}

impl NativeListArena {
    fn new(
        budget: *const Budget,
        max_collection_items: usize,
        output_limit_bytes: usize,
        initial_output_bytes: usize,
        profile: Option<Arc<RuntimeProfileCounters>>,
    ) -> Self {
        Self {
            float_lists: AddressedBoxes::new(),
            float_storage: Vec::new(),
            int_lists: AddressedBoxes::new(),
            inline_int_lists: Vec::new(),
            int_storage: Vec::new(),
            handle_lists: AddressedBoxes::new(),
            handle_storage: Vec::new(),
            native_records: Vec::new(),
            native_record_types: Vec::new(),
            native_record_fields: Vec::new(),
            native_record_indices: HashMap::new(),
            values: Vec::new(),
            constant_values: Arc::new(Vec::new()),
            inline_tagged_values: [NativeTaggedValue {
                kind: 0,
                payload_tag: 0,
                payload: 0,
                clone_charge_bytes: 0,
            }; NATIVE_INLINE_TAGGED_VALUES],
            inline_tagged_value_len: 0,
            tagged_values: Vec::new(),
            tagged_chunk_starts: std::collections::BTreeMap::new(),
            native_vm_continuations: Vec::new(),
            runtime_context: None,
            host_error: None,
            budget,
            profile,
            max_collection_items,
            charged_bytes: 0,
            output_limit_bytes,
            initial_output_bytes,
            committed_output_bytes: 0,
            pending_output: String::new(),
            pending_print_bytes: Vec::new(),
            reclaim_floor: NATIVE_ARENA_RECLAIM_MIN_ENTRIES,
            reclamations: 0,
        }
    }

    fn charge(&mut self, bytes: usize) -> bool {
        if self.budget.is_null() || !unsafe { (*self.budget).allocate(bytes) } {
            return false;
        }
        self.charged_bytes = self.charged_bytes.saturating_add(bytes);
        true
    }

    fn retained_entries(&self) -> usize {
        self.values
            .len()
            .saturating_add(self.handle_lists.len())
            .saturating_add(self.int_lists.len())
            .saturating_add(self.float_lists.len())
            .saturating_add(self.native_records.len())
            .saturating_add(self.inline_tagged_value_len)
            .saturating_add(
                self.tagged_values
                    .iter()
                    .map(Vec::len)
                    .fold(0usize, usize::saturating_add),
            )
    }

    fn discard_arena_handles(&mut self) {
        self.float_lists.clear();
        self.float_storage.clear();
        self.int_lists.clear();
        self.inline_int_lists.clear();
        self.int_storage.clear();
        self.handle_lists.clear();
        self.handle_storage.clear();
        self.native_records.clear();
        self.native_record_types.clear();
        self.native_record_fields.clear();
        self.native_record_indices.clear();
        self.values.clear();
        self.inline_tagged_value_len = 0;
        self.tagged_values.clear();
        self.tagged_chunk_starts.clear();
    }

    /// Retire every handle that is not reachable from the supplied live locals.
    ///
    /// `roots` is `(slot, tag, raw handle)` triples. Handles are materialized,
    /// the arena is emptied, and the live values are stored again; the raw
    /// words are rewritten in place. If any root cannot be evacuated the arena
    /// is left untouched. Guest allocation charges are cumulative and unchanged.
    fn reclaim_dead_handles(&mut self, roots: &mut [[i64; 3]]) -> bool {
        let retained = self.retained_entries();
        if retained < self.reclaim_floor || !self.native_vm_continuations.is_empty() {
            return false;
        }
        let mut live = Vec::with_capacity(roots.len());
        for root in roots.iter() {
            if !matches!(root[1], 4 | NATIVE_SAFEPOINT_TEXT_LIST_TAG)
                || root[2] >= NATIVE_CONSTANT_HANDLE_BASE
            {
                live.push(None);
                continue;
            }
            match self.materialize_native_value(root[2]) {
                Some(value) => live.push(Some(value)),
                None => return false,
            }
        }
        self.discard_arena_handles();
        for (root, value) in roots.iter_mut().zip(live) {
            let Some(value) = value else { continue };
            let stored = if root[1] == NATIVE_SAFEPOINT_TEXT_LIST_TAG {
                match value {
                    Value::List(items) => items
                        .iter()
                        .map(|item| match item {
                            Value::Text(text) => Some(SharedText::clone(text)),
                            _ => None,
                        })
                        .collect::<Option<Vec<_>>>()
                        .and_then(|items| self.store_native_text_list_charged(items, false)),
                    _ => None,
                }
            } else {
                self.encode_native_value(value)
            };
            match stored {
                Some(handle) => root[2] = handle,
                None => {
                    // Allocation of a handle slot failed after the arena was emptied.
                    self.host_error.get_or_insert_with(|| {
                        RuntimeError::new(
                            "R_ALLOCATION_LIMIT",
                            "native arena could not retain a live value",
                            Span::default(),
                        )
                    });
                    return true;
                }
            }
        }
        self.reclamations = self.reclamations.saturating_add(1);
        self.reclaim_floor = self
            .retained_entries()
            .saturating_mul(2)
            .max(NATIVE_ARENA_RECLAIM_MIN_ENTRIES);
        true
    }

    fn charge_or_error(&mut self, bytes: usize, span: Span) -> bool {
        if self.charge(bytes) {
            true
        } else {
            self.host_error = Some(if self.budget.is_null() {
                RuntimeError::new(
                    "R_ALLOCATION_LIMIT",
                    format!(
                        "cumulative allocation budget exhausted during this invocation{}",
                        guion_vm_core::ALLOCATION_LIMIT_HINT
                    ),
                    span,
                )
            } else {
                // SAFETY: a non-null budget outlives the native invocation using this arena.
                guion_vm_core::allocation_limit_error(unsafe { &*self.budget }, bytes, span)
            });
            false
        }
    }

    fn store_value(&mut self, value: Value) -> Option<i64> {
        self.values.try_reserve(1).ok()?;
        // These are backend handle-table entries. Their producer has already
        // charged any guest-visible value allocation; charging the handle slot
        // here would make the quota depend on whether the VM or JIT ran it.
        self.values.push(value);
        i64::try_from(self.values.len()).ok()
    }

    fn value(&self, handle: i64) -> Option<&Value> {
        let index = usize::try_from(handle.checked_sub(1)?).ok()?;
        self.values.get(index)
    }

    fn constant_value(&self, handle: i64) -> Option<&Value> {
        let index = usize::try_from(handle.checked_sub(NATIVE_CONSTANT_HANDLE_BASE)?).ok()?;
        self.constant_values.get(index)
    }

    fn handle_list(&self, handle: i64) -> Option<&NativeHandleList> {
        let address = usize::try_from(handle).ok()?;
        self.handle_lists.find_address(address)
    }

    fn native_record(&self, handle: i64) -> Option<(&NativeRecord, usize)> {
        let index = *self.native_record_indices.get(&handle)?;
        Some((
            self.native_records.get(index)?.as_ref(),
            *self.native_record_types.get(index)?,
        ))
    }

    fn float_list(&self, handle: i64) -> Option<&NativeFloatList> {
        let address = usize::try_from(handle).ok()?;
        self.float_lists.find_address(address)
    }

    fn int_list(&self, handle: i64) -> Option<&NativeIntList> {
        let address = usize::try_from(handle).ok()?;
        self.int_lists.find_address(address)
    }

    fn store_native_record_value(
        &mut self,
        record_index: usize,
        name: String,
        fields: Shared<Vec<Value>>,
    ) -> Option<i64> {
        let definition = self
            .runtime_context
            .as_ref()?
            .artifact
            .records
            .get(record_index)?
            .clone();
        let records = &self.runtime_context.as_ref()?.artifact.records;
        let words = native_record_words(&definition, records)?;
        if definition.name != name || fields.len() != definition.fields.len() {
            return None;
        }

        let mut encoded_fields = Vec::new();
        encoded_fields.try_reserve_exact(words).ok()?;
        for (value, field) in fields.iter().zip(&definition.fields) {
            if let Some(nested) = native_inline_record(&field.ty, records) {
                encode_inline_native_record(value, nested, records, &mut encoded_fields)?;
                continue;
            }
            let encoded = match (value, &field.ty) {
                (Value::Int(value), Type::Int) => value.to_i64()?,
                (Value::Float64(value), Type::Float64) => value.get().to_bits() as i64,
                (Value::Bool(value), Type::Bool) => i64::from(*value),
                (Value::Float64List(items), Type::List(item)) if **item == Type::Float64 => {
                    self.float_storage.try_reserve(1).ok()?;
                    self.float_lists.try_reserve(1).ok()?;
                    let storage_id = self.float_storage.len();
                    self.float_storage.push(items.as_ref().clone());
                    let storage = self.float_storage.last_mut()?;
                    let descriptor = Box::new(NativeFloatList {
                        data: storage.as_mut_ptr().cast::<u64>(),
                        len: storage.len(),
                        storage_id,
                        owned: false,
                        inline_storage: false,
                        inline: [0; NATIVE_INLINE_LIST_ITEMS],
                    });
                    let address = (&*descriptor as *const NativeFloatList) as usize;
                    self.float_lists.push(descriptor);
                    i64::try_from(address).ok()?
                }
                (Value::List(items), Type::List(item)) if **item == Type::Float64 => {
                    let values: Vec<Float64> = items
                        .iter()
                        .map(|value| match value {
                            Value::Float64(value) => Some(*value),
                            _ => None,
                        })
                        .collect::<Option<_>>()?;
                    if values.len() > self.max_collection_items {
                        return None;
                    }
                    self.float_storage.try_reserve(1).ok()?;
                    self.float_lists.try_reserve(1).ok()?;
                    let storage_id = self.float_storage.len();
                    self.float_storage.push(values);
                    let storage = self.float_storage.last_mut()?;
                    let descriptor = Box::new(NativeFloatList {
                        data: storage.as_mut_ptr().cast::<u64>(),
                        len: storage.len(),
                        storage_id,
                        owned: false,
                        inline_storage: false,
                        inline: [0; NATIVE_INLINE_LIST_ITEMS],
                    });
                    let address = (&*descriptor as *const NativeFloatList) as usize;
                    self.float_lists.push(descriptor);
                    i64::try_from(address).ok()?
                }
                (Value::List(items), Type::List(item))
                    if matches!(**item, Type::Int | Type::Bool) =>
                {
                    let values: Vec<i64> = items
                        .iter()
                        .map(|value| match (value, &**item) {
                            (Value::Int(value), Type::Int) => value.to_i64(),
                            (Value::Bool(value), Type::Bool) => Some(i64::from(*value)),
                            _ => None,
                        })
                        .collect::<Option<_>>()?;
                    if values.len() > self.max_collection_items {
                        return None;
                    }
                    self.int_storage.try_reserve(1).ok()?;
                    self.int_lists.try_reserve(1).ok()?;
                    let storage_id = self.int_storage.len();
                    self.int_storage.push(values);
                    let storage = self.int_storage.last_mut()?;
                    let descriptor = Box::new(NativeIntList {
                        data: storage.as_mut_ptr(),
                        len: storage.len(),
                        storage_id,
                        owned: false,
                        inline_storage: false,
                        inline: [0; NATIVE_INLINE_LIST_ITEMS],
                    });
                    let address = (&*descriptor as *const NativeIntList) as usize;
                    self.int_lists.push(descriptor);
                    i64::try_from(address).ok()?
                }
                _ => return None,
            };
            encoded_fields.push(encoded);
        }

        self.native_records.try_reserve(1).ok()?;
        self.native_record_types.try_reserve(1).ok()?;
        self.native_record_fields.try_reserve(1).ok()?;
        self.native_record_indices.try_reserve(1).ok()?;
        let fields = encoded_fields.into_boxed_slice();
        let descriptor = Box::new(NativeRecord {
            fields: fields.as_ptr(),
            len: fields.len(),
        });
        let address = (&*descriptor as *const NativeRecord) as usize;
        let handle = i64::try_from(address).ok()?;
        let index = self.native_records.len();
        self.native_record_fields.push(fields);
        self.native_records.push(descriptor);
        self.native_record_types.push(record_index);
        self.native_record_indices.insert(handle, index);
        Some(handle)
    }

    fn materialize_native_record(&self, handle: i64) -> Option<Value> {
        let (descriptor, record_index) = self.native_record(handle)?;
        self.materialize_native_record_descriptor(descriptor, record_index)
    }

    fn materialize_native_record_pointer(
        &self,
        pointer: i64,
        record_index: usize,
    ) -> Option<Value> {
        if let Some((descriptor, stored_record_index)) = self.native_record(pointer) {
            if stored_record_index != record_index {
                return None;
            }
            return self.materialize_native_record_descriptor(descriptor, record_index);
        }
        let address = usize::try_from(pointer).ok()?;
        if address == 0 {
            return None;
        }
        // Native-layout record pointers tagged by the compiler are either arena
        // descriptors above or stack descriptors emitted by MakeRecord. The
        // generated MapSet callback is synchronous, so the latter remains live
        // for this decode.
        let descriptor = unsafe { (address as *const NativeRecord).as_ref()? };
        self.materialize_native_record_descriptor(descriptor, record_index)
    }

    fn materialize_native_record_descriptor(
        &self,
        descriptor: &NativeRecord,
        record_index: usize,
    ) -> Option<Value> {
        let definition = self
            .runtime_context
            .as_ref()?
            .artifact
            .records
            .get(record_index)?;
        let records = &self.runtime_context.as_ref()?.artifact.records;
        if native_record_words(definition, records) != Some(descriptor.len)
            || (descriptor.len > 0 && descriptor.fields.is_null())
        {
            return None;
        }
        let encoded_fields = if descriptor.len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(descriptor.fields, descriptor.len) }
        };
        let mut fields = Vec::new();
        fields.try_reserve_exact(definition.fields.len()).ok()?;
        let mut offset = 0_usize;
        for field in &definition.fields {
            if let Some(nested) = native_inline_record(&field.ty, records) {
                let words = native_record_words(nested, records)?;
                fields.push(decode_inline_native_record(
                    encoded_fields.get(offset..offset.checked_add(words)?)?,
                    nested,
                    records,
                )?);
                offset += words;
                continue;
            }
            let encoded = encoded_fields.get(offset)?;
            offset += 1;
            let value = match &field.ty {
                Type::Int => Value::Int(BigInt::parse(&encoded.to_string()).ok()?),
                Type::Float64 => Value::Float64(Float64::new(f64::from_bits(*encoded as u64))?),
                Type::Bool => Value::Bool(*encoded != 0),
                Type::List(item) if **item == Type::Float64 => {
                    let list = if let Some(list) = self.float_list(*encoded) {
                        list
                    } else {
                        let address = usize::try_from(*encoded).ok()?;
                        if address == 0 {
                            return None;
                        }
                        unsafe { (address as *const NativeFloatList).as_ref()? }
                    };
                    if list.len > self.max_collection_items || (list.len > 0 && list.data.is_null())
                    {
                        return None;
                    }
                    let values = if list.len == 0 {
                        Vec::new()
                    } else {
                        unsafe { std::slice::from_raw_parts(list.data, list.len) }
                            .iter()
                            .map(|bits| Float64::new(f64::from_bits(*bits)).map(Value::Float64))
                            .collect::<Option<Vec<_>>>()?
                    };
                    Value::from_list_items(values)
                }
                Type::List(item) if **item == Type::Int || **item == Type::Bool => {
                    let list = if let Some(list) = self.int_list(*encoded) {
                        list
                    } else {
                        let address = usize::try_from(*encoded).ok()?;
                        if address == 0 {
                            return None;
                        }
                        unsafe { (address as *const NativeIntList).as_ref()? }
                    };
                    if list.len > self.max_collection_items || (list.len > 0 && list.data.is_null())
                    {
                        return None;
                    }
                    let values = if list.len == 0 {
                        Vec::new()
                    } else {
                        unsafe { std::slice::from_raw_parts(list.data, list.len) }
                            .iter()
                            .map(|value| {
                                if **item == Type::Int {
                                    BigInt::parse(&value.to_string()).ok().map(Value::Int)
                                } else {
                                    Some(Value::Bool(*value != 0))
                                }
                            })
                            .collect::<Option<Vec<_>>>()?
                    };
                    Value::from_list_items(values)
                }
                _ => return None,
            };
            fields.push(value);
        }
        Some(Value::Record(definition.name.clone(), Shared::new(fields)))
    }

    fn tagged_value(&self, handle: i64) -> Option<&NativeTaggedValue> {
        let address = usize::try_from(handle.checked_neg()?).ok()?;
        self.inline_tagged_values[..self.inline_tagged_value_len]
            .iter()
            .find(|value| (*value as *const NativeTaggedValue) as usize == address)
            .or_else(|| {
                let (start, chunk_index) =
                    self.tagged_chunk_starts.range(..=address).next_back()?;
                let offset = address - start;
                if offset % size_of::<NativeTaggedValue>() != 0 {
                    return None;
                }
                self.tagged_values
                    .get(*chunk_index)?
                    .get(offset / size_of::<NativeTaggedValue>())
            })
    }

    fn tagged_clone_charge_bytes(&mut self, handle: i64) -> Option<usize> {
        let address = usize::try_from(handle.checked_neg()?).ok()?;
        let descriptor = address as *mut NativeTaggedValue;
        // Negative handles are issued only for descriptors owned by this arena.
        // Inline slots and overflow chunks stay at fixed addresses for its lifetime.
        let descriptor_value = unsafe { descriptor.read() };
        if descriptor_value.clone_charge_bytes != 0 {
            return usize::try_from(descriptor_value.clone_charge_bytes).ok();
        }
        if descriptor_value.kind == NATIVE_TAGGED_OPTION_NONE {
            return Some(0);
        }
        let payload =
            self.decode_bridge_value(descriptor_value.payload, descriptor_value.payload_tag)?;
        let bytes = size_of::<Value>().saturating_add(cloned_value_allocation(&payload));
        let charge = u64::try_from(bytes).unwrap_or(u64::MAX);
        // The generated code has no Rust reference to this descriptor while the
        // callback runs; update only its cached quota field.
        unsafe { std::ptr::addr_of_mut!((*descriptor).clone_charge_bytes).write(charge) };
        Some(bytes)
    }

    fn native_value_handle_exists(&self, handle: i64) -> bool {
        self.constant_value(handle).is_some()
            || self.value(handle).is_some()
            || self.handle_list(handle).is_some()
            || self.native_record(handle).is_some()
            || self.tagged_value(handle).is_some()
    }

    fn ensure_tagged_capacity(&mut self) -> Option<()> {
        self.ensure_tagged_capacity_for(1)
    }

    fn ensure_tagged_capacity_for(&mut self, additional: usize) -> Option<()> {
        const TAGGED_VALUE_CHUNK_ITEMS: usize = 256;
        let inline_remaining =
            NATIVE_INLINE_TAGGED_VALUES.saturating_sub(self.inline_tagged_value_len);
        let overflow_needed = additional.saturating_sub(inline_remaining);
        let existing_overflow_capacity = self
            .tagged_values
            .last()
            .map_or(0, |chunk| chunk.capacity().saturating_sub(chunk.len()));
        let missing = overflow_needed.saturating_sub(existing_overflow_capacity);
        let chunks_needed = missing.div_ceil(TAGGED_VALUE_CHUNK_ITEMS);
        if chunks_needed == 0 {
            return Some(());
        }
        self.tagged_values.try_reserve(chunks_needed).ok()?;
        for _ in 0..chunks_needed {
            let mut chunk = Vec::new();
            chunk.try_reserve_exact(TAGGED_VALUE_CHUNK_ITEMS).ok()?;
            self.tagged_chunk_starts
                .insert(chunk.as_ptr() as usize, self.tagged_values.len());
            self.tagged_values.push(chunk);
        }
        Some(())
    }

    fn store_tagged_descriptor(&mut self, value: NativeTaggedValue) -> Option<i64> {
        if self.inline_tagged_value_len < NATIVE_INLINE_TAGGED_VALUES {
            let index = self.inline_tagged_value_len;
            self.inline_tagged_values[index] = value;
            self.inline_tagged_value_len += 1;
            let address = (&self.inline_tagged_values[index] as *const NativeTaggedValue) as usize;
            return i64::try_from(address).ok()?.checked_neg();
        }
        self.ensure_tagged_capacity()?;
        let chunk = self.tagged_values.last_mut()?;
        chunk.push(value);
        let address = (chunk.last()? as *const NativeTaggedValue) as usize;
        i64::try_from(address).ok()?.checked_neg()
    }

    fn encode_native_value(&mut self, value: Value) -> Option<i64> {
        match value {
            Value::ResultOk(value) => {
                self.store_tagged_value(NATIVE_TAGGED_RESULT_OK, Some(*value))
            }
            Value::ResultErr(value) => {
                self.store_tagged_value(NATIVE_TAGGED_RESULT_ERR, Some(*value))
            }
            Value::OptionSome(value) => {
                self.store_tagged_value(NATIVE_TAGGED_OPTION_SOME, Some(*value))
            }
            Value::OptionNone => self.store_tagged_value(NATIVE_TAGGED_OPTION_NONE, None),
            Value::Record(name, fields) => {
                let record_index = self
                    .runtime_context
                    .as_ref()?
                    .artifact
                    .records
                    .iter()
                    .position(|record| record.name == name)?;
                let records = &self.runtime_context.as_ref()?.artifact.records;
                if native_record_has_native_layout(records.get(record_index)?, records) {
                    self.store_native_record_value(record_index, name.into(), fields)
                } else {
                    self.store_value(Value::Record(name, fields))
                }
            }
            value => self.store_value(value),
        }
    }

    fn store_tagged_value(&mut self, kind: i64, payload: Option<Value>) -> Option<i64> {
        let (payload_tag, payload) = match payload {
            None => (0, 0),
            Some(Value::Int(value)) => match value.to_i64() {
                Some(value) => (1, value),
                None => (4, self.store_value(Value::Int(value))?),
            },
            Some(Value::Float64(value)) => (2, value.get().to_bits() as i64),
            Some(Value::Bool(value)) => (3, i64::from(value)),
            Some(value) => (4, self.encode_native_value(value)?),
        };
        self.store_tagged_descriptor(NativeTaggedValue {
            kind,
            payload_tag,
            payload,
            // The exact logical clone charge is needed only if guest code clones it.
            clone_charge_bytes: 0,
        })
    }

    fn store_native_tagged_value(
        &mut self,
        kind: i64,
        payload_tag: i64,
        payload: i64,
    ) -> Option<i64> {
        let has_payload = match kind {
            NATIVE_TAGGED_RESULT_OK | NATIVE_TAGGED_RESULT_ERR | NATIVE_TAGGED_OPTION_SOME => true,
            NATIVE_TAGGED_OPTION_NONE if payload_tag == 0 && payload == 0 => false,
            _ => return None,
        };
        if has_payload {
            let valid_payload = match payload_tag {
                1 => true,
                2 => Float64::new(f64::from_bits(payload as u64)).is_some(),
                3 => true,
                4 => self.native_value_handle_exists(payload),
                _ => false,
            };
            if !valid_payload {
                return None;
            }
        }
        if self.inline_tagged_value_len == NATIVE_INLINE_TAGGED_VALUES {
            self.ensure_tagged_capacity()?;
        }
        if has_payload && !self.charge(size_of::<Value>()) {
            return None;
        }
        let clone_charge_bytes = match (has_payload, payload_tag) {
            // Primitive payload clones have no guest allocation of their own;
            // the wrapper's allocation charge is fully known at construction.
            (true, 1..=3) => u64::try_from(size_of::<Value>()).ok()?,
            _ => 0,
        };
        self.store_tagged_descriptor(NativeTaggedValue {
            kind,
            payload_tag,
            payload,
            clone_charge_bytes,
        })
    }

    fn store_native_arguments(
        &mut self,
        arguments: &[String],
        max_text_bytes: usize,
    ) -> Option<i64> {
        if arguments.len() > self.max_collection_items
            || arguments
                .iter()
                .any(|argument| argument.len() > max_text_bytes)
        {
            return None;
        }

        let mut values = Vec::new();
        values.try_reserve_exact(arguments.len()).ok()?;
        for argument in arguments {
            values.push(Value::Text(argument.clone().into()));
        }

        let mut encoded = Vec::new();
        encoded.try_reserve_exact(arguments.len()).ok()?;
        let first_handle = self.values.len().checked_add(1)?;
        for index in 0..arguments.len() {
            encoded.push(i64::try_from(first_handle.checked_add(index)?).ok()?);
        }
        self.values.try_reserve(arguments.len()).ok()?;
        self.handle_lists.try_reserve(1).ok()?;
        let inline_storage = arguments.len() <= NATIVE_INLINE_LIST_ITEMS;
        if !inline_storage {
            self.handle_storage.try_reserve(1).ok()?;
        }

        let allocation = size_of::<Vec<Value>>()
            .saturating_add(arguments.len().saturating_mul(size_of::<Value>()))
            .saturating_add(arguments.iter().fold(0_usize, |total, argument| {
                total.saturating_add(size_of::<String>().saturating_add(argument.len()))
            }));
        let mut descriptor = Box::new(NativeHandleList {
            data: std::ptr::null_mut(),
            len: arguments.len(),
            storage_id: if inline_storage {
                usize::MAX
            } else {
                self.handle_storage.len()
            },
            owned: true,
            inline_storage,
            inline: [0; NATIVE_INLINE_LIST_ITEMS],
        });
        if inline_storage {
            descriptor.inline[..arguments.len()].copy_from_slice(&encoded);
            descriptor.data = descriptor.inline.as_mut_ptr();
        } else {
            descriptor.data = encoded.as_mut_ptr();
        }

        // If any check or quota charge fails, the caller side-exits before Args.
        // The VM then produces the canonical error and charges the allocation once.
        if !self.charge(allocation) {
            return None;
        }
        self.values.extend(values);
        if !inline_storage {
            self.handle_storage.push(encoded);
        }
        self.handle_lists.push(descriptor);
        let descriptor = self.handle_lists.last_mut()?;
        i64::try_from((&mut **descriptor as *mut NativeHandleList) as usize).ok()
    }

    fn store_native_text_list(&mut self, items: Vec<SharedText>) -> Option<i64> {
        self.store_native_text_list_charged(items, true)
    }

    /// `charge` is false only when re-storing a list whose allocation was already charged.
    fn store_native_text_list_charged(
        &mut self,
        items: Vec<SharedText>,
        charge: bool,
    ) -> Option<i64> {
        if items.len() > self.max_collection_items {
            return None;
        }
        let allocation = items.iter().fold(
            size_of::<Vec<Value>>().saturating_add(items.len().saturating_mul(size_of::<Value>())),
            |total, item| total.saturating_add(size_of::<String>().saturating_add(item.len())),
        );
        let mut values = Vec::new();
        values.try_reserve_exact(items.len()).ok()?;
        let mut encoded = Vec::new();
        encoded.try_reserve_exact(items.len()).ok()?;
        let first_handle = self.values.len().checked_add(1)?;
        for (index, item) in items.into_iter().enumerate() {
            encoded.push(i64::try_from(first_handle.checked_add(index)?).ok()?);
            values.push(Value::Text(item));
        }
        self.values.try_reserve(values.len()).ok()?;
        self.handle_lists.try_reserve(1).ok()?;
        let inline_storage = values.len() <= NATIVE_INLINE_LIST_ITEMS;
        if !inline_storage {
            self.handle_storage.try_reserve(1).ok()?;
        }
        let mut descriptor = Box::new(NativeHandleList {
            data: std::ptr::null_mut(),
            len: values.len(),
            storage_id: if inline_storage {
                usize::MAX
            } else {
                self.handle_storage.len()
            },
            owned: true,
            inline_storage,
            inline: [0; NATIVE_INLINE_LIST_ITEMS],
        });
        if inline_storage {
            descriptor.inline[..values.len()].copy_from_slice(&encoded);
            descriptor.data = descriptor.inline.as_mut_ptr();
        } else {
            descriptor.data = encoded.as_mut_ptr();
        }
        if charge && !self.charge(allocation) {
            return None;
        }
        self.values.extend(values);
        if !inline_storage {
            self.handle_storage.push(encoded);
        }
        self.handle_lists.push(descriptor);
        let descriptor = self.handle_lists.last_mut()?;
        i64::try_from((&mut **descriptor as *mut NativeHandleList) as usize).ok()
    }

    fn materialize_native_value(&self, handle: i64) -> Option<Value> {
        if let Some(value) = self.constant_value(handle) {
            return Some(value.clone());
        }
        if self.native_record(handle).is_some() {
            return self.materialize_native_record(handle);
        }
        if let Some(list) = self.handle_list(handle) {
            let encoded = unsafe { std::slice::from_raw_parts(list.data, list.len) };
            let values = encoded
                .iter()
                .map(|value| self.materialize_native_value(*value))
                .collect::<Option<Vec<_>>>()?;
            return Some(Value::List(Shared::new(values)));
        }
        let Some(tagged) = self.tagged_value(handle) else {
            return self.value(handle).cloned();
        };
        let (kind, payload_tag, payload) = (tagged.kind, tagged.payload_tag, tagged.payload);
        if kind == NATIVE_TAGGED_OPTION_NONE {
            return Some(Value::OptionNone);
        }
        let payload = match payload_tag {
            1 => Value::Int(BigInt::parse(&payload.to_string()).ok()?),
            2 => Value::Float64(Float64::new(f64::from_bits(payload as u64))?),
            3 => Value::Bool(payload != 0),
            4 => self.materialize_native_value(payload)?,
            _ => return None,
        };
        match kind {
            NATIVE_TAGGED_RESULT_OK => Some(Value::ResultOk(Box::new(payload))),
            NATIVE_TAGGED_RESULT_ERR => Some(Value::ResultErr(Box::new(payload))),
            NATIVE_TAGGED_OPTION_SOME => Some(Value::OptionSome(Box::new(payload))),
            _ => None,
        }
    }

    fn decode_continuation_value(&self, raw: i64, tag: i64) -> Option<Value> {
        match tag {
            0 => Some(Value::Unit),
            1 => Some(Value::Int(BigInt::parse(&raw.to_string()).ok()?)),
            2 => Some(Value::Float64(Float64::new(f64::from_bits(raw as u64))?)),
            3 => Some(Value::Bool(raw != 0)),
            4 => self.materialize_native_value(raw),
            _ => None,
        }
    }

    fn append_native_continuation(
        &mut self,
        frame: NativeVmContinuationFrame,
        caller_ip: Option<usize>,
    ) -> bool {
        if let Some(caller_ip) = caller_ip {
            let Some(child) = self.native_vm_continuations.last_mut() else {
                return false;
            };
            let Some(span) = self
                .runtime_context
                .as_ref()
                .and_then(|context| context.artifact.functions.get(frame.function_index))
                .and_then(|function| function.code.get(caller_ip))
                .map(|instruction| instruction.span)
            else {
                self.native_vm_continuations.clear();
                return false;
            };
            child.call_site = Some(span);
        } else if !self.native_vm_continuations.is_empty() {
            self.native_vm_continuations.clear();
            return false;
        }
        self.native_vm_continuations.push(frame);
        true
    }

    fn bridge_handle_argument<'a>(
        &'a self,
        arguments: &[i64],
        argument_tags: &[i64],
        index: usize,
    ) -> Option<&'a Value> {
        if *argument_tags.get(index)? != 4 {
            return None;
        }
        let handle = *arguments.get(index)?;
        self.constant_value(handle).or_else(|| self.value(handle))
    }

    fn decode_bridge_arguments(
        &self,
        arguments: &[i64],
        argument_tags: &[i64],
    ) -> Option<Vec<Value>> {
        arguments
            .iter()
            .zip(argument_tags)
            .map(|(value, tag)| self.decode_bridge_value(*value, *tag))
            .collect()
    }

    fn attach_runtime_context(
        &mut self,
        instance: *mut Instance,
        artifact: Arc<Artifact>,
        constants: Arc<Vec<Value>>,
        budget: Arc<Budget>,
        limits: VmLimits,
    ) {
        self.constant_values = constants;
        self.runtime_context = Some(NativeRuntimeContext {
            instance,
            artifact,
            budget,
            limits,
        });
    }

    fn execute_native_operation(&mut self, request: NativeOperationRequest<'_>) -> Option<i64> {
        let NativeOperationRequest {
            operation,
            function_index,
            instruction_index,
            depth,
            arguments,
            argument_tags,
            result_tag,
        } = request;
        if arguments.len() != argument_tags.len() {
            return None;
        }
        let runtime = self.runtime_context.as_ref()?.clone();
        let function_index = usize::try_from(function_index).ok()?;
        let instruction_index = usize::try_from(instruction_index).ok()?;
        let function = runtime.artifact.functions.get(function_index)?;
        let instruction = function.code.get(instruction_index)?;
        if native_backend::native_bridge_operation(&instruction.op)? != operation {
            return None;
        }
        let value = match &instruction.op {
            Op::Index => {
                if arguments.len() != 2 || argument_tags != [4, 4] {
                    return None;
                }
                let Value::Map(map) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let Value::Text(key) = self.bridge_handle_argument(arguments, argument_tags, 1)?
                else {
                    return None;
                };
                let value = map.get(key.as_ref())?.clone();
                if !self.charge(cloned_value_allocation(&value)) {
                    return None;
                }
                return self.encode_bridge_value(value, result_tag);
            }
            Op::MapSet => {
                if arguments.len() != 5
                    || argument_tags[0] != 4
                    || argument_tags[1] != 4
                    || (!matches!(argument_tags[2], 1..=4)
                        && native_backend::native_record_bridge_index(argument_tags[2]).is_none())
                    || argument_tags[3] != 4
                    || argument_tags[4] != 3
                    || result_tag != 4
                {
                    return None;
                }
                if !matches!(
                    self.bridge_handle_argument(arguments, argument_tags, 0)?,
                    Value::Map(_)
                ) {
                    return None;
                }
                let Value::Text(key) = self.bridge_handle_argument(arguments, argument_tags, 1)?
                else {
                    return None;
                };
                let key = key.to_string();
                let value = if let Some(record_index) =
                    native_backend::native_record_bridge_index(argument_tags[2])
                {
                    let record = runtime.artifact.records.get(record_index)?;
                    if !native_record_has_native_layout(record, &runtime.artifact.records) {
                        return None;
                    }
                    self.materialize_native_record_pointer(arguments[2], record_index)?
                } else if argument_tags[2] == 4 {
                    self.bridge_handle_argument(arguments, argument_tags, 2)?
                        .clone()
                } else {
                    self.decode_bridge_value(arguments[2], argument_tags[2])?
                };
                let unique_store_target =
                    depth == 0 && arguments[4] != 0 && arguments[3] == arguments[0];
                let map_handle = arguments[0];
                let map_value_index = map_handle
                    .checked_sub(1)
                    .and_then(|index| usize::try_from(index).ok());
                let map_is_mutable_handle =
                    map_value_index.is_some_and(|index| self.values.get(index).is_some());
                let (contains_key, map_len, unique, clone_allocation, cloned_map) = {
                    let Value::Map(map) =
                        self.bridge_handle_argument(arguments, argument_tags, 0)?
                    else {
                        return None;
                    };
                    let contains_key = map.contains_key(&key);
                    let unique = unique_store_target
                        && map_is_mutable_handle
                        && Shared::strong_count(map) == 1;
                    let clone_allocation = if unique {
                        if contains_key {
                            0
                        } else {
                            size_of::<(String, Value)>().saturating_add(key.len())
                        }
                    } else {
                        map_clone_allocation(map, Some(&key), &value)
                    };
                    let cloned_map = (!unique).then(|| map.as_ref().clone());
                    (
                        contains_key,
                        map.len(),
                        unique,
                        clone_allocation,
                        cloned_map,
                    )
                };
                if !contains_key && map_len >= runtime.limits.max_collection_items {
                    return None;
                }
                if key.len() > runtime.limits.max_text_bytes
                    || guion_vm_core::value_limit_error(&value, &runtime.limits, instruction.span)
                        .is_some()
                    || !self.charge(clone_allocation)
                {
                    return None;
                }
                if unique {
                    let index = map_value_index?;
                    let Value::Map(map) = self.values.get_mut(index)? else {
                        return None;
                    };
                    let values = Shared::get_mut(map)?;
                    if let Some(existing) = values.get_mut(&key) {
                        *existing = value;
                    } else {
                        values.insert(key, value);
                    }
                    return Some(map_handle);
                }
                let mut updated = cloned_map?;
                if let Some(existing) = updated.get_mut(&key) {
                    *existing = value;
                } else {
                    updated.insert(key, value);
                }
                return self.store_value(Value::Map(Shared::new(updated)));
            }
            Op::JsonSetLocal(_) => {
                if arguments.len() != 4 || argument_tags != [4, 4, 4, 3] || result_tag != 4 {
                    return None;
                }
                let profile = runtime.budget.runtime_profile.clone();
                let host_started = profile.as_ref().map(|_| Instant::now());
                let Value::Text(key) = self.bridge_handle_argument(arguments, argument_tags, 1)?
                else {
                    return None;
                };
                let key = key.to_string();
                let Value::Json(value) =
                    self.bridge_handle_argument(arguments, argument_tags, 2)?
                else {
                    return None;
                };
                let value = Shared::clone(value);
                if key.len() > runtime.limits.max_text_bytes {
                    return None;
                }

                let object_handle = arguments[0];
                let object_index = object_handle
                    .checked_sub(1)
                    .and_then(|index| usize::try_from(index).ok())
                    .filter(|index| self.values.get(*index).is_some());
                let unique_target = arguments[3] != 0 && object_index.is_some();
                let (field_count, contains_key, copied_bytes, shared) = {
                    let Value::Json(object) =
                        self.bridge_handle_argument(arguments, argument_tags, 0)?
                    else {
                        return None;
                    };
                    let fields = object.as_object()?;
                    let copied_bytes = if unique_target {
                        shared_json_set_local_allocation(object, &key)
                    } else {
                        // Native locals carry arena handles, so aliases do not
                        // increment the reference count as VM locals do. Add one
                        // logical reference while estimating the COW charge.
                        let aliased = Shared::clone(object);
                        shared_json_set_local_allocation(&aliased, &key)
                    };
                    let shared = !unique_target
                        || Shared::strong_count(object) > 1
                        || matches!(
                            object.as_ref(),
                            SharedJson::Object(values) if Shared::strong_count(values) > 1
                        );
                    (
                        fields.len(),
                        fields.contains_key(&key),
                        copied_bytes,
                        shared,
                    )
                };
                if !contains_key && field_count >= runtime.limits.max_collection_items {
                    return None;
                }
                if !self.charge(copied_bytes) {
                    return None;
                }

                let updated_handle = if unique_target {
                    let index = object_index?;
                    let Value::Json(object) = self.values.get_mut(index)? else {
                        return None;
                    };
                    Shared::make_mut(object)
                        .as_object_mut()?
                        .insert(key.clone(), value);
                    object_handle
                } else {
                    let Value::Json(mut object) = self
                        .bridge_handle_argument(arguments, argument_tags, 0)?
                        .clone()
                    else {
                        return None;
                    };
                    Shared::make_mut(&mut object)
                        .as_object_mut()?
                        .insert(key.clone(), value);
                    self.store_value(Value::Json(object))?
                };

                if let Some(error) = self
                    .bridge_handle_argument(&[updated_handle], &[4], 0)
                    .and_then(|updated| {
                        guion_vm_core::value_limit_error(updated, &runtime.limits, instruction.span)
                    })
                {
                    self.host_error = Some(error);
                    record_native_host_operation(
                        profile.as_ref(),
                        host_started,
                        opcode_profile_name(&instruction.op),
                    );
                    return None;
                }
                if let Some(profile) = &profile {
                    profile.record_json_set(if shared { copied_bytes } else { 0 });
                }
                record_native_host_operation(
                    profile.as_ref(),
                    host_started,
                    opcode_profile_name(&instruction.op),
                );
                return Some(updated_handle);
            }
            Op::JsonPushLocal(_) => {
                if arguments.len() != 3 || argument_tags != [4, 4, 3] || result_tag != 4 {
                    return None;
                }
                let profile = runtime.budget.runtime_profile.clone();
                let host_started = profile.as_ref().map(|_| Instant::now());
                let Value::Json(value) =
                    self.bridge_handle_argument(arguments, argument_tags, 1)?
                else {
                    return None;
                };
                let value = Shared::clone(value);

                let array_handle = arguments[0];
                let array_index = array_handle
                    .checked_sub(1)
                    .and_then(|index| usize::try_from(index).ok())
                    .filter(|index| self.values.get(*index).is_some());
                let unique_target = arguments[2] != 0 && array_index.is_some();
                let (array_len, copied_bytes) = {
                    let Value::Json(array) =
                        self.bridge_handle_argument(arguments, argument_tags, 0)?
                    else {
                        return None;
                    };
                    let items = array.as_array()?;
                    let copied_bytes = if unique_target {
                        shared_json_push_local_allocation(array)
                    } else {
                        // Arena handles do not increase reference counts for aliases
                        // held in native locals. Model the VM's shared local.
                        let aliased = Shared::clone(array);
                        shared_json_push_local_allocation(&aliased)
                    };
                    (items.len(), copied_bytes)
                };
                if array_len >= runtime.limits.max_collection_items {
                    return None;
                }
                if !self.charge(copied_bytes) {
                    return None;
                }

                let updated_handle = if unique_target {
                    let index = array_index?;
                    let Value::Json(array) = self.values.get_mut(index)? else {
                        return None;
                    };
                    Shared::make_mut(array).as_array_mut()?.push(value);
                    array_handle
                } else {
                    let Value::Json(mut array) = self
                        .bridge_handle_argument(arguments, argument_tags, 0)?
                        .clone()
                    else {
                        return None;
                    };
                    Shared::make_mut(&mut array).as_array_mut()?.push(value);
                    self.store_value(Value::Json(array))?
                };

                if let Some(error) = self
                    .bridge_handle_argument(&[updated_handle], &[4], 0)
                    .and_then(|updated| {
                        guion_vm_core::value_limit_error(updated, &runtime.limits, instruction.span)
                    })
                {
                    self.host_error = Some(error);
                    record_native_host_operation(
                        profile.as_ref(),
                        host_started,
                        opcode_profile_name(&instruction.op),
                    );
                    return None;
                }
                record_native_host_operation(
                    profile.as_ref(),
                    host_started,
                    opcode_profile_name(&instruction.op),
                );
                return Some(updated_handle);
            }
            Op::JsonInt => {
                if arguments.len() != 1 || argument_tags != [1] || result_tag != 4 {
                    return None;
                }
                let integer = BigInt::from_i64(arguments[0]);
                if integer.digit_count() > runtime.limits.max_integer_digits {
                    return None;
                }
                let parsed =
                    serde_json::from_str::<serde_json::Value>(&integer.to_string()).ok()?;
                let value = match finish_json_value(
                    parsed,
                    &runtime.limits,
                    &runtime.budget,
                    instruction.span,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        self.host_error = Some(error);
                        return None;
                    }
                };
                return self.store_value(value);
            }
            Op::MapHas => {
                if arguments.len() != 2 || argument_tags != [4, 4] || result_tag != 3 {
                    return None;
                }
                let Value::Map(map) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let Value::Text(key) = self.bridge_handle_argument(arguments, argument_tags, 1)?
                else {
                    return None;
                };
                return Some(i64::from(map.contains_key(key.as_ref())));
            }
            Op::JsonText => {
                if arguments.len() != 1 || argument_tags != [4] || result_tag != 4 {
                    return None;
                }
                let Value::Text(text) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                if text.len() > runtime.limits.max_text_bytes {
                    return None;
                }
                let value = match finish_json_value(
                    serde_json::Value::String(text.to_string()),
                    &runtime.limits,
                    &runtime.budget,
                    instruction.span,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        self.host_error = Some(error);
                        return None;
                    }
                };
                return self.store_value(value);
            }
            Op::JsonObject => {
                if !arguments.is_empty() || result_tag != 4 {
                    return None;
                }
                let value = match finish_json_value(
                    serde_json::Value::Object(serde_json::Map::new()),
                    &runtime.limits,
                    &runtime.budget,
                    instruction.span,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        self.host_error = Some(error);
                        return None;
                    }
                };
                return self.store_value(value);
            }
            Op::Add => {
                if arguments.len() != 2 || argument_tags != [4, 4] || result_tag != 4 {
                    return None;
                }
                let Value::Text(left) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let left = left.clone();
                let Value::Text(right) =
                    self.bridge_handle_argument(arguments, argument_tags, 1)?
                else {
                    return None;
                };
                let right = right.clone();
                let text_bytes = left.len().saturating_add(right.len());
                if text_bytes > runtime.limits.max_text_bytes {
                    return None;
                }
                let allocation = size_of::<String>().saturating_add(text_bytes);
                if !self.charge(allocation) {
                    return None;
                }
                let mut text = String::with_capacity(text_bytes);
                text.push_str(&left);
                text.push_str(&right);
                return self.store_value(Value::Text(text.into()));
            }
            Op::Equal | Op::NotEqual => {
                if arguments.len() != 2 || argument_tags != [4, 4] || result_tag != 3 {
                    return None;
                }
                let Value::Text(left) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let Value::Text(right) =
                    self.bridge_handle_argument(arguments, argument_tags, 1)?
                else {
                    return None;
                };
                let equal = left == right;
                return Some(i64::from(if matches!(&instruction.op, Op::Equal) {
                    equal
                } else {
                    !equal
                }));
            }
            Op::PrintErr => {
                if arguments.len() != 1 || argument_tags != [4] || result_tag != 4 {
                    return None;
                }
                let Value::Text(text) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let text = text.clone();
                let output_bytes = text.len().saturating_add(1);
                let instance = unsafe { &mut *runtime.instance };
                if instance
                    .program_output_bytes()
                    .saturating_add(self.pending_output.len())
                    .saturating_add(output_bytes)
                    > self.output_limit_bytes
                    || !self.charge(output_bytes)
                {
                    return None;
                }
                let mut rendered = String::with_capacity(output_bytes);
                rendered.push_str(&text);
                rendered.push('\n');
                instance.commit_native_output(self);
                instance
                    .write_program_output(OutputStream::Stderr, &rendered, instruction.span)
                    .ok()?;
                self.committed_output_bytes =
                    self.committed_output_bytes.saturating_add(rendered.len());
                return Some(0);
            }
            Op::Args => {
                if result_tag != 4 || !arguments.is_empty() {
                    return None;
                }
                let instance = unsafe { &*runtime.instance };
                return self.store_native_arguments(
                    &instance.policy.arguments,
                    instance.policy.max_text_bytes,
                );
            }
            Op::MakeTypedMap(_) => {
                if !arguments.is_empty() || result_tag != 4 {
                    return None;
                }
                let allocation = TextMap::ACCOUNTED_HEADER_BYTES;
                if !self.charge(allocation) {
                    return None;
                }
                return self.store_value(Value::Map(Shared::new(TextMap::new())));
            }
            Op::MapKeys => {
                if arguments.len() != 1 || argument_tags != [4] || result_tag != 4 {
                    return None;
                }
                let Value::Map(map) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let mut keys = Vec::new();
                keys.try_reserve_exact(map.len()).ok()?;
                keys.extend(map.keys().cloned());
                return self.store_native_text_list(keys);
            }
            Op::TextChars => {
                if arguments.len() != 1 || argument_tags != [4] || result_tag != 4 {
                    return None;
                }
                let Value::Text(text) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let character_count = text.char_count();
                if character_count > self.max_collection_items {
                    return None;
                }
                let mut characters = Vec::new();
                characters.try_reserve_exact(character_count).ok()?;
                characters.extend(
                    text.chars()
                        .map(|character| SharedText::from(character.to_string())),
                );
                return self.store_native_text_list(characters);
            }
            Op::TextSlice => {
                if arguments.len() != 3 || argument_tags != [4, 1, 1] || result_tag != 4 {
                    return None;
                }
                let Value::Text(text) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let text = SharedText::clone(text);
                let start = usize::try_from(arguments[1]).ok()?;
                let end = usize::try_from(arguments[2]).ok()?;
                if start > end {
                    return None;
                }

                let mut scalar_index = 0;
                let mut byte_start = (start == 0).then_some(0);
                let mut byte_end = (end == 0).then_some(0);
                let mut reached_end = false;
                for (byte_index, _) in text.char_indices() {
                    if scalar_index == start {
                        byte_start = Some(byte_index);
                    }
                    if scalar_index == end {
                        byte_end = Some(byte_index);
                        reached_end = true;
                        break;
                    }
                    scalar_index += 1;
                }
                if byte_start.is_none() && start == scalar_index {
                    byte_start = Some(text.len());
                }
                if byte_end.is_none() && end == scalar_index {
                    byte_end = Some(text.len());
                }
                if end > scalar_index {
                    return None;
                }
                let byte_start = byte_start?;
                let byte_end = byte_end?;

                self.values.try_reserve(1).ok()?;
                let value = if start == 0 && !reached_end {
                    Value::Text(text)
                } else {
                    let sliced = SharedText::from(&text[byte_start..byte_end]);
                    if !self.charge(size_of::<String>().saturating_add(sliced.len())) {
                        return None;
                    }
                    Value::Text(sliced)
                };
                return self.store_value(value);
            }
            Op::Length => {
                if arguments.len() != 1 || argument_tags != [4] || result_tag != 1 {
                    return None;
                }
                let Value::Text(text) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                return i64::try_from(text.char_count()).ok();
            }
            Op::TextFind => {
                if arguments.len() != 3 || argument_tags != [4, 4, 1] || result_tag != 1 {
                    return None;
                }
                let Value::Text(text) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let Value::Text(needle) =
                    self.bridge_handle_argument(arguments, argument_tags, 1)?
                else {
                    return None;
                };
                let from = usize::try_from(arguments[2]).ok()?;
                // A start past the end replays in the VM, which reports R_INDEX_BOUNDS.
                return match guion_vm_core::text_find_scalar(text, needle, from)? {
                    Some(index) => i64::try_from(index).ok(),
                    None => Some(-1),
                };
            }
            Op::TextSplit => {
                if arguments.len() != 2 || argument_tags != [4, 4] || result_tag != 4 {
                    return None;
                }
                let Value::Text(text) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let Value::Text(delimiter) =
                    self.bridge_handle_argument(arguments, argument_tags, 1)?
                else {
                    return None;
                };
                let fields: Vec<SharedText> = if delimiter.is_empty() {
                    vec![SharedText::clone(text)]
                } else {
                    let offsets = guion_vm_core::text_match_offsets(
                        text,
                        delimiter,
                        self.max_collection_items.saturating_sub(1),
                    )?;
                    let mut fields = Vec::with_capacity(offsets.len() + 1);
                    let mut start = 0;
                    for offset in offsets {
                        fields.push(SharedText::from(&text[start..offset]));
                        start = offset + delimiter.len();
                    }
                    fields.push(SharedText::from(&text[start..]));
                    fields
                };
                return self.store_native_text_list(fields);
            }
            Op::TextReplace => {
                if arguments.len() != 3 || argument_tags != [4, 4, 4] || result_tag != 4 {
                    return None;
                }
                let Value::Text(text) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let Value::Text(needle) =
                    self.bridge_handle_argument(arguments, argument_tags, 1)?
                else {
                    return None;
                };
                let Value::Text(replacement) =
                    self.bridge_handle_argument(arguments, argument_tags, 2)?
                else {
                    return None;
                };
                let count = if needle.is_empty() {
                    0
                } else {
                    guion_vm_core::text_match_count(text, needle)
                };
                let value = if count == 0 {
                    Value::Text(SharedText::clone(text))
                } else {
                    let total = (text.len() - count * needle.len())
                        .saturating_add(count.saturating_mul(replacement.len()));
                    if total > runtime.limits.max_text_bytes {
                        return None;
                    }
                    let replaced =
                        guion_vm_core::text_replace_all(text, needle, replacement, total);
                    if !self.charge(total) {
                        return None;
                    }
                    Value::Text(replaced)
                };
                self.values.try_reserve(1).ok()?;
                return self.store_value(value);
            }
            Op::TextTrim => {
                if arguments.len() != 1 || argument_tags != [4] || result_tag != 4 {
                    return None;
                }
                let Value::Text(text) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let trimmed = text.trim();
                let value = if trimmed.len() == text.len() {
                    Value::Text(SharedText::clone(text))
                } else {
                    let trimmed = SharedText::from(trimmed);
                    if !self.charge(size_of::<String>().saturating_add(trimmed.len())) {
                        return None;
                    }
                    Value::Text(trimmed)
                };
                self.values.try_reserve(1).ok()?;
                return self.store_value(value);
            }
            Op::TextJoin => {
                if arguments.len() != 2 || argument_tags != [4, 4] || result_tag != 4 {
                    return None;
                }
                let Value::List(values) = self.materialize_native_value(arguments[0])? else {
                    return None;
                };
                let Value::Text(delimiter) =
                    self.bridge_handle_argument(arguments, argument_tags, 1)?
                else {
                    return None;
                };
                let joined = guion_vm_core::text_join_checked(
                    &values,
                    delimiter,
                    runtime.limits.max_collection_items,
                    runtime.limits.max_text_bytes,
                )?;
                if !self.charge(joined.len()) {
                    return None;
                }
                return self.store_value(Value::Text(SharedText::from(joined)));
            }
            Op::JsonGet => {
                if arguments.len() != 2 || argument_tags != [4, 4] || result_tag != 4 {
                    return None;
                }
                let Value::Json(document) =
                    self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let Value::Text(key) = self.bridge_handle_argument(arguments, argument_tags, 1)?
                else {
                    return None;
                };
                let value = document.as_object()?.get(key.as_ref())?.clone();
                return self.store_value(Value::Json(value));
            }
            Op::JsonStringify => {
                if arguments.len() != 1 || argument_tags != [4] || result_tag != 4 {
                    return None;
                }
                let Value::Json(value) =
                    self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let text = serde_json::to_string(value.as_ref()).ok()?;
                if text.len() > runtime.limits.max_text_bytes {
                    return None;
                }
                let allocation = size_of::<String>().saturating_add(text.len());
                if !self.charge(allocation) {
                    return None;
                }
                return self.store_value(Value::Text(text.into()));
            }
            Op::JsonParse | Op::JsonTryGet | Op::JsonTryText | Op::JsonTryInt => {
                return self.execute_native_json_operation(NativeJsonOperationRequest {
                    operation,
                    arguments,
                    argument_tags,
                    result_tag,
                    span: instruction.span,
                    operation_name: opcode_profile_name(&instruction.op),
                    runtime: &runtime,
                });
            }
            Op::FileNextLine => {
                return self.execute_native_file_next_line(
                    arguments,
                    argument_tags,
                    result_tag,
                    instruction.span,
                    opcode_profile_name(&instruction.op),
                    &runtime,
                );
            }
            Op::FileOpenLines => {
                if arguments.len() != 1 || argument_tags != [4] || result_tag != 4 {
                    return None;
                }
                let profile = runtime.budget.runtime_profile.clone();
                let host_started = profile.as_ref().map(|_| Instant::now());
                let Value::Text(path) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let path = path.to_string();
                self.values.try_reserve(1).ok()?;
                self.ensure_tagged_capacity_for(1)?;
                let instance = unsafe { runtime.instance.as_mut()? };
                let resolved =
                    match host_file_path(&instance.policy, &path, false, instruction.span) {
                        Ok(resolved) => resolved,
                        Err(error) => {
                            let value =
                                self.encode_bridge_value(file_error_result(error), result_tag);
                            record_native_host_operation(
                                profile.as_ref(),
                                host_started,
                                opcode_profile_name(&instruction.op),
                            );
                            return value;
                        }
                    };
                let effect = instance.begin_effect(EffectKind::FileRead, path.as_str());
                let result = match instance.readers.open_lines(resolved) {
                    Ok(handle) => {
                        instance.finish_effect(effect, EffectOutcome::Returned);
                        let (owner, id) = handle.parts();
                        Value::ResultOk(Box::new(Value::NativeResource(Box::new(
                            NativeResourceValue::new("FileReader", owner, id),
                        ))))
                    }
                    Err(error) => {
                        instance.finish_effect(effect, EffectOutcome::ReturnedError);
                        let code = match error.kind {
                            stdlib_data::ReaderErrorKind::InvalidUtf8 => "R_FILE_UTF8",
                            stdlib_data::ReaderErrorKind::Limit => "R_FILE_LIMIT",
                            stdlib_data::ReaderErrorKind::ForeignOwner => "R_RESOURCE_OWNER",
                            stdlib_data::ReaderErrorKind::Closed => "R_RESOURCE_CLOSED",
                            stdlib_data::ReaderErrorKind::Stale => "R_RESOURCE_STALE",
                            stdlib_data::ReaderErrorKind::Mode => "R_RESOURCE_MODE",
                            stdlib_data::ReaderErrorKind::Io => "R_FILE_READ",
                        };
                        file_error_result(RuntimeError::new(code, error.message, instruction.span))
                    }
                };
                let value = self.encode_bridge_value(result, result_tag);
                record_native_host_operation(
                    profile.as_ref(),
                    host_started,
                    opcode_profile_name(&instruction.op),
                );
                return value;
            }
            Op::FileClose => {
                if arguments.len() != 1 || argument_tags != [4] || result_tag != 4 {
                    return None;
                }
                let profile = runtime.budget.runtime_profile.clone();
                let host_started = profile.as_ref().map(|_| Instant::now());
                let value = self.bridge_handle_argument(arguments, argument_tags, 0)?;
                let handle = match runtime_reader_handle(value, instruction.span) {
                    Ok(handle) => handle,
                    Err(error) => {
                        self.host_error = Some(error);
                        record_native_host_operation(
                            profile.as_ref(),
                            host_started,
                            opcode_profile_name(&instruction.op),
                        );
                        return None;
                    }
                };
                self.values.try_reserve(1).ok()?;
                self.ensure_tagged_capacity_for(1)?;
                let instance = unsafe { runtime.instance.as_mut()? };
                let result = match instance.readers.close(handle) {
                    Ok(()) => Value::ResultOk(Box::new(Value::Unit)),
                    Err(error) => {
                        let error = reader_runtime_error(
                            error.kind,
                            error.message,
                            error.line,
                            error.column,
                            instruction.span,
                        );
                        if error.code == "R_FILE_LIMIT" {
                            self.host_error = Some(error);
                            record_native_host_operation(
                                profile.as_ref(),
                                host_started,
                                opcode_profile_name(&instruction.op),
                            );
                            return None;
                        }
                        file_error_result(error)
                    }
                };
                let value = self.encode_bridge_value(result, result_tag);
                record_native_host_operation(
                    profile.as_ref(),
                    host_started,
                    opcode_profile_name(&instruction.op),
                );
                return value;
            }
            Op::TextCollapseWhitespace => {
                if arguments.len() != 1 || argument_tags != [4] || result_tag != 4 {
                    return None;
                }
                let Value::Text(text) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let Some(output) = stdlib_services::text_collapse_whitespace_if_changed(text)
                else {
                    return Some(arguments[0]);
                };
                if output.len() > runtime.limits.max_text_bytes || !self.charge(output.len()) {
                    return None;
                }
                return self.store_value(Value::Text(output.into()));
            }
            Op::TimeParseTimestamp => {
                if arguments.len() != 1 || argument_tags != [4] || result_tag != 4 {
                    return None;
                }
                let profile = runtime.budget.runtime_profile.clone();
                let host_started = profile.as_ref().map(|_| Instant::now());
                let Value::Text(text) = self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                if text.len() > runtime.limits.max_text_bytes {
                    return None;
                }
                let parsed = stdlib_data::parse_timestamp(text);
                let result = match parsed {
                    Ok(microseconds) => Value::ResultOk(Box::new(Value::Record(
                        "Instant".into(),
                        Shared::new(vec![Value::Int(BigInt::from_i64(microseconds))]),
                    ))),
                    Err(error) => file_error_result(RuntimeError::new(
                        error.code,
                        error.message,
                        instruction.span,
                    )),
                };
                let value = self.encode_bridge_value(result, result_tag);
                record_native_host_operation(
                    profile.as_ref(),
                    host_started,
                    opcode_profile_name(&instruction.op),
                );
                return value;
            }
            Op::TimeFormatUtc => {
                if arguments.len() != 1 || argument_tags != [4] || result_tag != 4 {
                    return None;
                }
                let instant_microseconds = if let Some(Value::Record(name, fields)) =
                    self.bridge_handle_argument(arguments, argument_tags, 0)
                {
                    if name != "Instant" || fields.len() != 1 {
                        return None;
                    }
                    let Value::Int(microseconds) = &fields[0] else {
                        return None;
                    };
                    microseconds.to_i64()?
                } else {
                    let address = usize::try_from(arguments[0]).ok()?;
                    if address == 0 {
                        return None;
                    }
                    // Native-layout Instant records are passed as stable descriptors
                    // owned by the active JIT invocation. Other representations use
                    // arena handles and are decoded by bridge_handle_argument above.
                    let record = unsafe { (address as *const NativeRecord).as_ref()? };
                    if record.len != 1 || record.fields.is_null() {
                        return None;
                    }
                    unsafe { *record.fields }
                };
                let text = stdlib_data::format_utc(stdlib_data::Instant {
                    unix_microseconds: instant_microseconds,
                });
                if text.len() > runtime.limits.max_text_bytes {
                    return None;
                }
                let allocation = size_of::<String>().saturating_add(text.len());
                if !self.charge(allocation) {
                    return None;
                }
                return self.store_value(Value::Text(text.into()));
            }
            Op::GetField(record_index, field_index) => {
                let Value::Record(name, fields) =
                    self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let definition = runtime.artifact.records.get(*record_index)?;
                if name != &definition.name {
                    return None;
                }
                let field = fields.get(*field_index)?.clone();
                if !self.charge_or_error(cloned_value_allocation(&field), instruction.span) {
                    return None;
                }
                field
            }
            Op::MakeRecord(record_index, count) => {
                if *count != arguments.len() {
                    return None;
                }
                let values = self.decode_bridge_arguments(arguments, argument_tags)?;
                let definition = runtime.artifact.records.get(*record_index)?;
                let charge_bytes = size_of::<Vec<Value>>()
                    .saturating_add(values.len().saturating_mul(size_of::<Value>()))
                    .saturating_add(definition.name.len());
                if !self.charge_or_error(charge_bytes, instruction.span) {
                    return None;
                }
                Value::Record(definition.name.clone(), Shared::new(values))
            }
            _ => {
                let mut stack = self.decode_bridge_arguments(arguments, argument_tags)?;
                // These bridged host operations do not access guest locals. Avoid
                // allocating a frame-sized local vector for each native callback.
                let mut locals = Vec::new();
                let allocations_before = runtime.budget.allocation.remaining();
                // This native invocation is synchronous. NativeCode is held through a
                // cloned Arc, so the callback is the only Instance access while JIT
                // code is running.
                let result = unsafe {
                    VmHost::host_op(
                        &mut *runtime.instance,
                        HostOpContext {
                            instruction,
                            function,
                            stack: &mut stack,
                            locals: &mut locals,
                            budget: &runtime.budget,
                            depth: usize::try_from(depth).ok()?,
                        },
                    )
                };
                let allocations_after = runtime.budget.allocation.remaining();
                self.charged_bytes = self
                    .charged_bytes
                    .saturating_add(allocations_before.saturating_sub(allocations_after));
                if let Err(error) = result {
                    self.host_error = Some(error);
                    return None;
                }
                if stack.len() != 1 {
                    return None;
                }
                return self.encode_bridge_value(stack.pop()?, result_tag);
            }
        };
        self.encode_bridge_value(value, result_tag)
    }

    fn execute_native_json_operation(
        &mut self,
        request: NativeJsonOperationRequest<'_>,
    ) -> Option<i64> {
        let NativeJsonOperationRequest {
            operation,
            arguments,
            argument_tags,
            result_tag,
            span,
            operation_name,
            runtime,
        } = request;
        if result_tag != 4 {
            return None;
        }
        // Reserve backend bookkeeping before parsing or charging guest output.
        self.values.try_reserve(1).ok()?;
        if self.inline_tagged_value_len == NATIVE_INLINE_TAGGED_VALUES {
            self.ensure_tagged_capacity_for(1)?;
        }
        let profile = runtime.budget.runtime_profile.clone();
        let host_started = profile.as_ref().map(|_| Instant::now());
        let (kind, payload) = match operation {
            native_backend::NATIVE_BRIDGE_JSON_PARSE => {
                if arguments.len() != 1 || argument_tags != [4] {
                    return None;
                }
                let Value::Text(source) =
                    self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                if source.len() > runtime.limits.max_text_bytes {
                    let constant_span = arguments
                        .first()
                        .and_then(|handle| handle.checked_sub(NATIVE_CONSTANT_HANDLE_BASE))
                        .and_then(|index| usize::try_from(index).ok())
                        .and_then(|index| {
                            runtime
                                .artifact
                                .functions
                                .iter()
                                .flat_map(|function| function.code.iter())
                                .filter(|instruction| {
                                    matches!(
                                        &instruction.op,
                                        Op::Const(Value::Text(_) | Value::Bytes(_))
                                    )
                                })
                                .nth(index)
                                .map(|instruction| instruction.span)
                        });
                    let constant_error = constant_span.and_then(|constant_span| {
                        guion_vm_core::value_limit_error_at_boundary(
                            &Value::Text(source.clone()),
                            &runtime.limits,
                            constant_span,
                        )
                    });
                    self.host_error = Some(constant_error.unwrap_or_else(|| {
                        RuntimeError::new(
                            "R_TEXT_LIMIT",
                            "JSON input exceeds the configured byte limit",
                            span,
                        )
                    }));
                    record_native_host_operation(profile.as_ref(), host_started, operation_name);
                    return None;
                }
                let parse_started = runtime
                    .budget
                    .runtime_profile
                    .as_ref()
                    .map(|_| Instant::now());
                let parsed = parse_unique_shared_json(source);
                if let (Some(profile), Some(started)) =
                    (&runtime.budget.runtime_profile, parse_started)
                {
                    profile.record_json_parse(
                        started.elapsed().as_nanos().min(u64::MAX as u128) as u64
                    );
                }
                match parsed {
                    Ok(value) => {
                        let allocations_before = runtime.budget.allocation.remaining();
                        let parsed = finish_shared_json(
                            value,
                            &runtime.limits,
                            runtime.budget.as_ref(),
                            span,
                        );
                        let allocations_after = runtime.budget.allocation.remaining();
                        self.charged_bytes = self
                            .charged_bytes
                            .saturating_add(allocations_before.saturating_sub(allocations_after));
                        match parsed {
                            Ok(Value::Json(value)) => {
                                (native_backend::NATIVE_TAGGED_RESULT_OK, Value::Json(value))
                            }
                            Ok(_) => return None,
                            Err(error) => {
                                self.host_error = Some(error);
                                record_native_host_operation(
                                    profile.as_ref(),
                                    host_started,
                                    operation_name,
                                );
                                return None;
                            }
                        }
                    }
                    Err(JsonDecodeError::Parse(error)) => (
                        native_backend::NATIVE_TAGGED_RESULT_ERR,
                        Value::JsonError(Box::new(JsonErrorValue {
                            code: "R_JSON_PARSE".into(),
                            message: error.to_string(),
                            line: error.line(),
                            column: error.column(),
                        })),
                    ),
                    Err(JsonDecodeError::DuplicateKey { key, line, column }) => (
                        native_backend::NATIVE_TAGGED_RESULT_ERR,
                        Value::JsonError(Box::new(JsonErrorValue {
                            code: "R_JSON_DUPLICATE_KEY".into(),
                            message: format!("JSON object contains duplicate key `{key}`"),
                            line,
                            column,
                        })),
                    ),
                }
            }
            native_backend::NATIVE_BRIDGE_JSON_TRY_GET => {
                if arguments.len() != 2 || argument_tags != [4, 4] {
                    return None;
                }
                let Value::Json(document) =
                    self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                let Value::Text(key) = self.bridge_handle_argument(arguments, argument_tags, 1)?
                else {
                    return None;
                };
                if key.len() > runtime.limits.max_text_bytes {
                    return None;
                }
                match stdlib_data::json_try_get(document, key) {
                    Ok(value) => (native_backend::NATIVE_TAGGED_RESULT_OK, Value::Json(value)),
                    Err(error) => (
                        native_backend::NATIVE_TAGGED_RESULT_ERR,
                        Value::JsonError(Box::new(error)),
                    ),
                }
            }
            native_backend::NATIVE_BRIDGE_JSON_TRY_TEXT => {
                if arguments.len() != 1 || argument_tags != [4] {
                    return None;
                }
                let Value::Json(document) =
                    self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                match stdlib_data::json_try_text(document.as_ref()) {
                    Ok(text) => (
                        native_backend::NATIVE_TAGGED_RESULT_OK,
                        Value::Text(text.into()),
                    ),
                    Err(error) => (
                        native_backend::NATIVE_TAGGED_RESULT_ERR,
                        Value::JsonError(Box::new(error)),
                    ),
                }
            }
            native_backend::NATIVE_BRIDGE_JSON_TRY_INT => {
                if arguments.len() != 1 || argument_tags != [4] {
                    return None;
                }
                let Value::Json(document) =
                    self.bridge_handle_argument(arguments, argument_tags, 0)?
                else {
                    return None;
                };
                match stdlib_data::json_try_int(document.as_ref()) {
                    Ok(integer) if integer.digit_count() <= runtime.limits.max_integer_digits => {
                        (native_backend::NATIVE_TAGGED_RESULT_OK, Value::Int(integer))
                    }
                    Ok(_) => return None,
                    Err(error) => (
                        native_backend::NATIVE_TAGGED_RESULT_ERR,
                        Value::JsonError(Box::new(error)),
                    ),
                }
            }
            _ => return None,
        };

        if let Value::Text(text) = &payload {
            let bytes = size_of::<String>().saturating_add(text.len());
            if !self.charge(bytes) {
                return None;
            }
        }
        let value = self.store_tagged_value(kind, Some(payload))?;
        record_native_host_operation(profile.as_ref(), host_started, operation_name);
        Some(value)
    }

    fn execute_native_file_next_line(
        &mut self,
        arguments: &[i64],
        argument_tags: &[i64],
        result_tag: i64,
        span: Span,
        operation_name: &'static str,
        runtime: &NativeRuntimeContext,
    ) -> Option<i64> {
        if result_tag != 4 || arguments.len() != 1 || argument_tags != [4] {
            return None;
        }
        let profile = runtime.budget.runtime_profile.clone();
        let host_started = profile.as_ref().map(|_| Instant::now());
        let value = self.bridge_handle_argument(arguments, argument_tags, 0)?;
        let handle = match runtime_reader_handle(value, span) {
            Ok(handle) => handle,
            Err(error) => {
                record_native_host_operation(profile.as_ref(), host_started, operation_name);
                self.host_error = Some(error);
                return None;
            }
        };

        // A file read advances the reader. Reserve all result storage first so
        // arena pressure cannot force a VM retry after consuming a line.
        self.values.try_reserve(1).ok()?;
        self.ensure_tagged_capacity_for(2)?;
        let line = unsafe { runtime.instance.as_mut()? }
            .readers
            .next_line(handle);
        let result = match line {
            Ok(Some(line)) => {
                Value::ResultOk(Box::new(Value::OptionSome(Box::new(Value::Record(
                    source_line_record_name(),
                    Shared::new(vec![
                        Value::Int(BigInt::from_usize(line.number)),
                        Value::Text(line.text),
                    ]),
                )))))
            }
            Ok(None) => Value::ResultOk(Box::new(Value::OptionNone)),
            Err(error) => file_error_result(reader_runtime_error(
                error.kind,
                error.message,
                error.line,
                error.column,
                span,
            )),
        };
        let value = self.encode_bridge_value(result, result_tag)?;
        record_native_host_operation(profile.as_ref(), host_started, operation_name);
        Some(value)
    }

    fn decode_bridge_value(&self, value: i64, tag: i64) -> Option<Value> {
        match tag {
            1 => Some(Value::Int(BigInt::parse(&value.to_string()).ok()?)),
            2 => Some(Value::Float64(Float64::new(f64::from_bits(value as u64))?)),
            3 => Some(Value::Bool(value != 0)),
            4 => self.materialize_native_value(value),
            _ => {
                let record_index = native_backend::native_record_bridge_index(tag)?;
                let records = &self.runtime_context.as_ref()?.artifact.records;
                if !native_record_has_native_layout(records.get(record_index)?, records) {
                    return None;
                }
                self.materialize_native_record_pointer(value, record_index)
            }
        }
    }

    fn encode_bridge_value(&mut self, value: Value, tag: i64) -> Option<i64> {
        match (tag, value) {
            (1, Value::Int(value)) => value.to_i64(),
            (2, Value::Float64(value)) => Some(value.get().to_bits() as i64),
            (3, Value::Bool(value)) => Some(i64::from(value)),
            // Storing a handle is backend bookkeeping. Guest allocation limits
            // are charged by the operation that produced `value`.
            (4, value) => self.encode_native_value(value),
            _ => None,
        }
    }

    fn record_bridge_panic(&mut self, function_index: i64, instruction_index: i64) {
        if self.host_error.is_some() {
            return;
        }
        let span = usize::try_from(function_index)
            .ok()
            .and_then(|function_index| {
                let context = self.runtime_context.as_ref()?;
                let function = context.artifact.functions.get(function_index)?;
                function
                    .code
                    .get(usize::try_from(instruction_index).ok()?)
                    .map(|instruction| instruction.span)
            })
            .unwrap_or_default();
        self.host_error = Some(RuntimeError::new(
            "R_NATIVE_BRIDGE",
            "native value bridge failed while executing a verified operation",
            span,
        ));
    }

    fn allocate_inline_int(
        &mut self,
        source: &[i64],
        index: usize,
        value: i64,
    ) -> *mut std::ffi::c_void {
        const CHUNK_SIZE: usize = 256;
        let mut inline = [0; NATIVE_INLINE_LIST_ITEMS];
        inline[..source.len()].copy_from_slice(source);
        inline[index] = value;
        if self
            .inline_int_lists
            .last()
            .is_none_or(|chunk| chunk.len() == chunk.capacity())
        {
            self.inline_int_lists.push(Vec::with_capacity(CHUNK_SIZE));
        }
        // Each chunk is filled only up to its fixed capacity. Moving its Vec
        // header never moves the backing allocation referenced by JIT code.
        let chunk = self.inline_int_lists.last_mut().unwrap();
        chunk.push(NativeIntList {
            data: std::ptr::null_mut(),
            len: source.len(),
            storage_id: usize::MAX,
            owned: true,
            inline_storage: true,
            inline,
        });
        let descriptor = chunk.last_mut().unwrap();
        descriptor.data = descriptor.inline.as_mut_ptr();
        (descriptor as *mut NativeIntList).cast()
    }

    fn record_list_set_operation(&self) {
        if let Some(profile) = &self.profile {
            profile.record_list_set_operation();
        }
    }

    fn record_list_set_in_place_update(&self) {
        if let Some(profile) = &self.profile {
            profile.record_list_set_in_place_update();
        }
    }

    fn record_list_set_copy(&self, elements: usize, bytes: usize) {
        if let Some(profile) = &self.profile {
            profile.record_list_set_copy(elements, bytes);
        }
    }

    fn record_record_charge(&self, bytes: usize) {
        if let Some(profile) = &self.profile {
            profile.record_record_charge(bytes);
        }
    }

    fn record_guest_conditional_branch(&self) {
        if let Some(profile) = &self.profile {
            profile.record_guest_conditional_branch();
        }
    }

    fn print(&mut self, value: i64, kind: i64) -> bool {
        let rendered = match kind {
            1 => value.to_string(),
            2 => {
                let Some(value) = Float64::new(f64::from_bits(value as u64)) else {
                    return false;
                };
                value.get().to_string()
            }
            3 if value == 0 => "false".to_owned(),
            3 if value == 1 => "true".to_owned(),
            4 => match self.constant_value(value).or_else(|| self.value(value)) {
                Some(Value::Text(text)) => text.to_string(),
                _ => return false,
            },
            _ => return false,
        };
        let bytes = rendered.len().saturating_add(1);
        if self
            .initial_output_bytes
            .saturating_add(self.committed_output_bytes)
            .saturating_add(self.pending_output.len())
            .saturating_add(bytes)
            > self.output_limit_bytes
            || !self.charge(bytes)
        {
            return false;
        }
        self.pending_output.push_str(&rendered);
        self.pending_output.push('\n');
        self.pending_print_bytes.push(bytes);
        true
    }
}

const MAX_EFFECT_REPORT_ENTRIES: usize = 1024;
const MAX_EFFECT_TARGET_CHARS: usize = 256;
const DEFAULT_ALLOCATION_LIMIT_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectKind {
    ProgramOutput,
    ProgramErrorOutput,
    FileRead,
    FileWrite,
    HttpRequest,
    PythonCall,
    EnvironmentRead,
    ClockRead,
    ProcessSpawn,
    NetworkSocket,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectOutcome {
    Started,
    Returned,
    ReturnedError,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectRecord {
    pub kind: EffectKind,
    pub target: String,
    pub target_truncated: bool,
    pub outcome: EffectOutcome,
}

#[derive(Clone, Debug)]
pub struct HostPolicy {
    pub instruction_limit: u64,
    /// Cumulative estimated guest-data allocations allowed during one invocation.
    /// Interpreter/JIT frames, operand stacks, and scalar arithmetic temporaries are excluded.
    pub allocation_limit_bytes: usize,
    pub call_depth_limit: usize,
    pub output_limit_bytes: usize,
    pub max_integer_digits: usize,
    pub max_text_bytes: usize,
    pub max_collection_items: usize,
    pub max_parallelism: usize,
    pub file_root: Option<PathBuf>,
    pub additional_file_roots: Vec<PathBuf>,
    /// Virtual path prefixes mapped to independently granted, root-confined file areas.
    pub file_root_aliases: HashMap<String, PathBuf>,
    pub allow_file_read: bool,
    pub allow_file_write: bool,
    pub max_file_bytes: usize,
    pub arguments: Vec<String>,
    pub allowed_environment_variables: HashSet<String>,
    /// Whether wall-clock reads are explicitly granted to the guest.
    pub allow_clock: bool,
    /// Optional fixed Unix-millisecond value for deterministic embedders and tests.
    pub clock_unix_time_ms_override: Option<i64>,
    #[cfg(feature = "python-interop")]
    pub python_interpreter: Option<PathBuf>,
    #[cfg(feature = "python-interop")]
    pub python_timeout: Duration,
    pub allowed_http_hosts: HashSet<String>,
    pub http_timeout: Duration,
    pub max_http_request_bytes: usize,
    pub max_http_response_bytes: usize,
    pub max_http_redirects: usize,
    /// Exact command names `process_run` may start; empty denies every command.
    pub allowed_commands: HashSet<String>,
    pub process_timeout: Duration,
    pub max_process_output_bytes: usize,
    /// Exact `host:port` (or `host:*`) entries sockets may connect or send to.
    pub allowed_connect: HashSet<String>,
    /// Exact `host:port` (or `host:*`) entries sockets may bind.
    pub allowed_bind: HashSet<String>,
    pub max_socket_timeout: Duration,
    pub max_socket_read_bytes: usize,
}
impl Default for HostPolicy {
    fn default() -> Self {
        Self {
            instruction_limit: 2_000_000,
            allocation_limit_bytes: DEFAULT_ALLOCATION_LIMIT_BYTES,
            call_depth_limit: 128,
            output_limit_bytes: 1_000_000,
            max_integer_digits: 4096,
            max_text_bytes: 16 * 1024 * 1024,
            max_collection_items: 1_000_000,
            max_parallelism: thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1)
                .min(64),
            file_root: None,
            additional_file_roots: Vec::new(),
            file_root_aliases: HashMap::new(),
            allow_file_read: false,
            allow_file_write: false,
            max_file_bytes: 16 * 1024 * 1024,
            arguments: Vec::new(),
            allowed_environment_variables: HashSet::new(),
            allow_clock: false,
            clock_unix_time_ms_override: None,
            #[cfg(feature = "python-interop")]
            python_interpreter: None,
            #[cfg(feature = "python-interop")]
            python_timeout: Duration::from_secs(15),
            allowed_http_hosts: HashSet::new(),
            http_timeout: Duration::from_secs(15),
            max_http_request_bytes: 1024 * 1024,
            max_http_response_bytes: 16 * 1024 * 1024,
            max_http_redirects: 5,
            allowed_commands: HashSet::new(),
            process_timeout: Duration::from_secs(30),
            max_process_output_bytes: 16 * 1024 * 1024,
            allowed_connect: HashSet::new(),
            allowed_bind: HashSet::new(),
            max_socket_timeout: Duration::from_secs(30),
            max_socket_read_bytes: 1024 * 1024,
        }
    }
}
impl HostPolicy {
    pub fn with_file_access(mut self, root: impl Into<PathBuf>, read: bool, write: bool) -> Self {
        self.file_root = Some(root.into());
        self.allow_file_read = read;
        self.allow_file_write = write;
        self
    }
    /// Add a secondary relative-path root. Reads search the primary root first;
    /// writes and Python bridge relative paths use the last granted root.
    pub fn with_additional_file_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.additional_file_roots.push(root.into());
        self
    }
    /// Map a virtual relative-path prefix (for example `std` or `pkg:codec`)
    /// to its own granted root. Paths under that prefix never fall through to
    /// the project root or another additional root.
    pub fn with_file_root_alias(
        mut self,
        alias: impl Into<String>,
        root: impl Into<PathBuf>,
    ) -> Self {
        self.file_root_aliases.insert(alias.into(), root.into());
        self
    }
    pub fn with_arguments(mut self, arguments: Vec<String>) -> Self {
        self.arguments = arguments;
        self
    }
    pub fn with_clock_access(mut self) -> Self {
        self.allow_clock = true;
        self
    }
    /// Grant clock access and provide a fixed Unix-millisecond value.
    pub fn with_clock_unix_time_ms(mut self, unix_time_ms: i64) -> Self {
        self.allow_clock = true;
        self.clock_unix_time_ms_override = Some(unix_time_ms);
        self
    }
    #[cfg(feature = "python-interop")]
    pub fn with_python(mut self, interpreter: impl Into<PathBuf>, timeout: Duration) -> Self {
        self.python_interpreter = Some(interpreter.into());
        self.python_timeout = timeout;
        self
    }
    pub fn with_network_hosts<I, S>(
        mut self,
        hosts: I,
        timeout: Duration,
        max_response_bytes: usize,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.allowed_http_hosts = hosts
            .into_iter()
            .map(|host| host.as_ref().trim_end_matches('.').to_ascii_lowercase())
            .collect();
        self.http_timeout = timeout;
        self.max_http_response_bytes = max_response_bytes;
        self
    }
}

pub struct Runtime {
    native_enabled: bool,
    native_whole_program_entry: bool,
    runtime_profile_enabled: bool,
    runtime_profile_sampled: bool,
}

fn current_unix_time_ms() -> Result<i64, String> {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis())
            .map_err(|_| "system clock is outside the supported millisecond range".into()),
        Err(error) => {
            let milliseconds = i64::try_from(error.duration().as_millis()).map_err(|_| {
                "system clock is outside the supported millisecond range".to_string()
            })?;
            Ok(-milliseconds)
        }
    }
}

#[cfg(unix)]
fn local_date_from_unix_time_ms(unix_time_ms: i64) -> Result<(i32, i32, i32), String> {
    let seconds = unix_time_ms.div_euclid(1_000);
    let timestamp = libc::time_t::try_from(seconds)
        .map_err(|_| "timestamp is outside the host local-time range".to_string())?;
    // localtime_r writes to the supplied tm and does not expose shared static storage.
    let mut local = unsafe { std::mem::zeroed::<libc::tm>() };
    if unsafe { libc::localtime_r(&timestamp, &mut local) }.is_null() {
        return Err("host could not convert the timestamp to local time".into());
    }
    let year = local
        .tm_year
        .checked_add(1_900)
        .ok_or_else(|| "local year is outside the supported range".to_string())?;
    let month = local
        .tm_mon
        .checked_add(1)
        .ok_or_else(|| "local month is outside the supported range".to_string())?;
    Ok((year, month, local.tm_mday))
}

#[cfg(not(unix))]
fn local_date_from_unix_time_ms(_unix_time_ms: i64) -> Result<(i32, i32, i32), String> {
    Err("local date lookup is supported on Unix hosts only".into())
}

/// The text and byte constants native code addresses by handle. They depend only on the
/// artifact, so an instance builds the table once, not on every native entry.
fn native_constant_table(artifact: &Artifact) -> Arc<Vec<Value>> {
    #[cfg(test)]
    ARTIFACT_SCAN_TALLY.with(|scans| scans.set(scans.get() + 1));
    Arc::new(
        artifact
            .functions
            .iter()
            .flat_map(|function| function.code.iter())
            .filter_map(|instruction| match &instruction.op {
                Op::Const(value @ (Value::Text(_) | Value::Bytes(_))) => Some(value.clone()),
                _ => None,
            })
            .collect(),
    )
}

fn share_artifact(artifact: Artifact) -> Arc<Artifact> {
    Arc::new(artifact)
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}
impl Runtime {
    pub fn new() -> Self {
        Self {
            native_enabled: true,
            native_whole_program_entry: false,
            runtime_profile_enabled: false,
            runtime_profile_sampled: false,
        }
    }

    /// Create a runtime that always uses the verified bytecode VM.
    /// This is useful for reproducible backend comparisons and diagnosis.
    pub fn vm_only() -> Self {
        Self {
            native_enabled: false,
            native_whole_program_entry: false,
            runtime_profile_enabled: false,
            runtime_profile_sampled: false,
        }
    }

    /// Enable diagnostic execution counters for the latest guest invocation.
    /// This adds instrumentation and should be used only for untimed profiles.
    /// The first function invoked is the only one this runtime will invoke, as for a program
    /// run from its entry point, so nothing it cannot reach needs native code.
    pub fn with_whole_program_entry(mut self) -> Self {
        self.native_whole_program_entry = true;
        self
    }

    pub fn with_runtime_profile(mut self) -> Self {
        self.runtime_profile_enabled = true;
        self.runtime_profile_sampled = false;
        self
    }

    /// Enable sampled attribution with reduced per-instruction instrumentation.
    pub fn with_sampled_runtime_profile(mut self) -> Self {
        self.runtime_profile_enabled = true;
        self.runtime_profile_sampled = true;
        self
    }
    pub fn load(
        &self,
        artifact: Artifact,
        mut policy: HostPolicy,
    ) -> Result<Instance, RuntimeError> {
        verify(&artifact)?;
        if let Some(metadata) = &artifact.build_metadata {
            let limits = &metadata.resource_limits;
            policy.instruction_limit = policy.instruction_limit.min(limits.instruction_limit);
            policy.allocation_limit_bytes = policy
                .allocation_limit_bytes
                .min(limits.allocation_limit_bytes);
            policy.call_depth_limit = policy.call_depth_limit.min(limits.call_depth_limit);
            policy.output_limit_bytes = policy.output_limit_bytes.min(limits.output_limit_bytes);
            policy.max_integer_digits = policy.max_integer_digits.min(limits.max_integer_digits);
            policy.max_text_bytes = policy.max_text_bytes.min(limits.max_text_bytes);
            policy.max_collection_items =
                policy.max_collection_items.min(limits.max_collection_items);
            policy.max_parallelism = policy.max_parallelism.min(limits.max_parallelism);
            policy.max_file_bytes = policy.max_file_bytes.min(limits.max_file_bytes);
        }
        let mut database_roots = Vec::with_capacity(
            1 + policy.additional_file_roots.len() + policy.file_root_aliases.len(),
        );
        if let Some(root) = &policy.file_root {
            database_roots.push(root.clone());
        }
        database_roots.extend(policy.additional_file_roots.iter().cloned());
        database_roots.extend(policy.file_root_aliases.values().cloned());
        let mut artifact = artifact;
        inline_host_forwarders(&mut artifact);
        let registers = Arc::new(build_register_programs(&artifact));
        let native_constants = native_constant_table(&artifact);
        let artifact = share_artifact(artifact);
        Ok(Instance {
            artifact,
            native_constants,
            registers,
            native: None,
            native_code: None,
            native_enabled: self.native_enabled,
            native_whole_program_entry: self.native_whole_program_entry,
            native_compile_attempted: false,
            runtime_profile_enabled: self.runtime_profile_enabled,
            runtime_profile_sampled: self.runtime_profile_sampled,
            runtime_profile_metrics: None,
            native_calls: Arc::new(AtomicUsize::new(0)),
            native_deoptimizations: Arc::new(AtomicUsize::new(0)),
            native_deoptimization_sites: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
            native_compilations: Arc::new(AtomicUsize::new(0)),
            native_entry_copy_dominated: Vec::new(),
            readers: stdlib_data::FileReaderRegistry::new(usize::MAX, policy.max_file_bytes),
            sockets: net_ops::SocketTable::default(),
            databases: stdlib_services::DatabaseRegistry::with_roots(
                database_roots,
                policy.max_file_bytes,
                policy.max_collection_items,
                2_000,
            ),
            http: None,
            toml_documents: stdlib_services::TomlRegistry::new(policy.max_text_bytes),
            policy,
            output: String::new(),
            program_stderr: String::new(),
            closed: false,
            worker_threads: HashSet::new(),
            cancelled: None,
            #[cfg(feature = "python-interop")]
            python_bridge: None,
            host_stderr: String::new(),
            effect_report: Vec::new(),
            omitted_effects: 0,
            python_native_image_observation: "not_used".into(),
            python_native_images: Vec::new(),
        })
    }
}

#[derive(Debug)]
pub struct Instance {
    artifact: Arc<Artifact>,
    native_constants: Arc<Vec<Value>>,
    registers: Arc<RegisterPrograms>,
    native: Option<NativeBackend>,
    native_code: Option<Arc<NativeCode>>,
    native_enabled: bool,
    native_whole_program_entry: bool,
    native_compile_attempted: bool,
    runtime_profile_enabled: bool,
    runtime_profile_sampled: bool,
    runtime_profile_metrics: Option<Arc<RuntimeProfileCounters>>,
    native_calls: Arc<AtomicUsize>,
    native_deoptimizations: Arc<AtomicUsize>,
    native_deoptimization_sites: Arc<std::sync::Mutex<BTreeMap<(usize, usize), usize>>>,
    native_compilations: Arc<AtomicUsize>,
    /// Per function, how many native entries copied far more list items than they used.
    native_entry_copy_dominated: Vec<u8>,
    policy: HostPolicy,
    readers: stdlib_data::FileReaderRegistry,
    databases: stdlib_services::DatabaseRegistry,
    sockets: net_ops::SocketTable,
    http: Option<Arc<HostHttpClient>>,
    toml_documents: stdlib_services::TomlRegistry,
    output: String,
    program_stderr: String,
    closed: bool,
    worker_threads: HashSet<String>,
    cancelled: Option<Arc<AtomicBool>>,
    #[cfg(feature = "python-interop")]
    python_bridge: Option<python_bridge::PythonBridge>,
    host_stderr: String,
    effect_report: Vec<EffectRecord>,
    omitted_effects: usize,
    python_native_image_observation: String,
    python_native_images: Vec<String>,
}

impl HostPolicy {
    fn vm_limits(&self) -> VmLimits {
        VmLimits {
            allocation_limit_bytes: self.allocation_limit_bytes,
            call_depth_limit: self.call_depth_limit,
            output_limit_bytes: self.output_limit_bytes,
            max_integer_digits: self.max_integer_digits,
            max_text_bytes: self.max_text_bytes,
            max_collection_items: self.max_collection_items,
            max_file_bytes: self.max_file_bytes,
        }
    }
}

/// Replaces calls to tiny forwarding functions (`Load 0..n; <host operation>; Return`) with
/// the host operation itself. Instruction indices, jump targets and spans are unchanged and
/// the stack effect is identical, so the VM and the native backend both see the direct op.
/// Readers build one of these records per row, so each name is allocated only once.
fn source_line_record_name() -> guion_model::RecordName {
    static NAME: std::sync::OnceLock<guion_model::RecordName> = std::sync::OnceLock::new();
    NAME.get_or_init(|| guion_model::RecordName::immortal("SourceLine"))
        .clone()
}

fn csv_column_record_name() -> guion_model::RecordName {
    static NAME: std::sync::OnceLock<guion_model::RecordName> = std::sync::OnceLock::new();
    NAME.get_or_init(|| guion_model::RecordName::immortal("CsvColumn"))
        .clone()
}

fn csv_column_value(column: stdlib_data::csv_columns::Column) -> Value {
    use guion_model::{ArrayData, ArrayValue};
    let array = |data: ArrayData| Value::Array(Shared::new(ArrayValue::vector(data)));
    Value::Record(
        csv_column_record_name(),
        Shared::new(vec![
            Value::Text(column.name.into()),
            Value::Text(column.kind.name().into()),
            array(ArrayData::I64(column.ints)),
            array(ArrayData::F64(column.floats)),
            array(ArrayData::Bool(column.flags)),
            array(ArrayData::I64(column.codes)),
            Value::from_list_items(
                column
                    .dictionary
                    .into_iter()
                    .map(|text| Value::Text(text.into()))
                    .collect(),
            ),
            array(ArrayData::Bool(column.valid)),
        ]),
    )
}

fn csv_row_record_name() -> guion_model::RecordName {
    static NAME: std::sync::OnceLock<guion_model::RecordName> = std::sync::OnceLock::new();
    NAME.get_or_init(|| guion_model::RecordName::immortal("CsvRow"))
        .clone()
}

fn inline_host_forwarders(artifact: &mut Artifact) {
    let operations: Vec<Option<Op>> = artifact
        .functions
        .iter()
        .map(|function| {
            guion_vm_core::forwarded_host_operation(function)
                .filter(|operation| !matches!(operation, Op::MakeSome | Op::MakeNone))
                .or_else(|| forwarded_text_operation(function))
        })
        .collect();
    if operations.iter().all(Option::is_none) {
        return;
    }
    for function in &mut artifact.functions {
        for instruction in &mut function.code {
            if let Op::Call(target, _) = &instruction.op {
                if let Some(Some(operation)) = operations.get(*target) {
                    instruction.op = operation.clone();
                }
            }
        }
    }
}

/// The text operation a `std:text` wrapper such as `text_split` applies to its own
/// parameters, when the function is exactly `Load 0..n; <operation>; Return`. Inlining it
/// lets native callers use the operation directly instead of calling into the VM.
fn forwarded_text_operation(function: &guion_model::Function) -> Option<Op> {
    let arity = function.parameters.len();
    let operation = &function.code.get(arity)?.op;
    let expected = match operation {
        Op::TextFind | Op::TextReplace => 3,
        Op::TextSplit | Op::TextJoin => 2,
        Op::TextTrim => 1,
        _ => return None,
    };
    let loads_parameters = function.code[..arity]
        .iter()
        .enumerate()
        .all(|(slot, instruction)| matches!(instruction.op, Op::Load(loaded) if loaded == slot));
    (arity == expected
        && function.local_count == arity
        && function.code.len() == arity + 2
        && loads_parameters
        && matches!(function.code[arity + 1].op, Op::Return))
    .then(|| operation.clone())
}

fn value_limit_error(value: &Value, policy: &HostPolicy, span: Span) -> Option<RuntimeError> {
    guion_vm_core::value_limit_error(value, &policy.vm_limits(), span)
}

fn consume_sort_step(budget: &Budget, span: Span) -> Result<(), RuntimeError> {
    if budget.is_cancelled() {
        return Err(RuntimeError::new(
            "R_CANCELLED",
            "parallel task was cancelled after a sibling failed",
            span,
        ));
    }
    if !budget.consume() {
        return Err(RuntimeError::new(
            "R_STEP_LIMIT",
            "instruction budget exhausted",
            span,
        ));
    }
    Ok(())
}

fn value_limit_error_at_boundary(
    value: &Value,
    policy: &HostPolicy,
    span: Span,
) -> Option<RuntimeError> {
    guion_vm_core::value_limit_error_at_boundary(value, &policy.vm_limits(), span)
}

#[inline(never)]
fn http_response_text_value(
    response: Value,
    budget: &Budget,
    max_text_bytes: usize,
    span: Span,
) -> Result<Value, RuntimeError> {
    let fields = match response {
        Value::Record(name, fields) if name == "HttpResponse" && fields.len() == 3 => fields,
        _ => {
            return Err(RuntimeError::new(
                "R_TYPE_INVALID",
                "http_response_text argument must be HttpResponse",
                span,
            ));
        }
    };
    let Value::Bytes(body) = &fields[2] else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "HttpResponse body must be Bytes",
            span,
        ));
    };
    if body.len() > max_text_bytes {
        let mut error = http_error(
            "R_HTTP_LIMIT",
            "HTTP response exceeds the configured text byte limit",
        );
        error.trace.push(span);
        return Ok(Value::ResultErr(Box::new(Value::HttpError(Box::new(
            error,
        )))));
    }
    match std::str::from_utf8(body.as_ref()) {
        Ok(text) => {
            charge_allocation(budget, text.len(), span)?;
            Ok(Value::ResultOk(Box::new(Value::Text(text.into()))))
        }
        Err(_) => {
            let mut error = http_error("R_HTTP_ENCODING", "HTTP response body is not valid UTF-8");
            error.trace.push(span);
            Ok(Value::ResultErr(Box::new(Value::HttpError(Box::new(
                error,
            )))))
        }
    }
}

fn std_error_record_value(
    artifact: &Artifact,
    budget: &Budget,
    code: &str,
    message: &str,
    span: Span,
) -> Result<Value, RuntimeError> {
    let record = artifact
        .records
        .iter()
        .find(|record| record.name == "StdError")
        .ok_or_else(|| {
            RuntimeError::new(
                "R_ARTIFACT_INVALID",
                "HTTP JSON response requires the StdError record definition",
                span,
            )
        })?;
    if record.fields.len() != 5 {
        return Err(RuntimeError::new(
            "R_ARTIFACT_INVALID",
            "StdError must contain code, message, path, line, and column fields",
            span,
        ));
    }
    let record_bytes = size_of::<Vec<Value>>()
        .saturating_add(record.fields.len().saturating_mul(size_of::<Value>()))
        .saturating_add(record.name.len());
    charge_allocation(budget, record_bytes, span)?;
    budget.record_record_charge(record_bytes);
    Ok(Value::Record(
        record.name.clone(),
        Shared::new(vec![
            Value::Text(SharedText::from(code)),
            Value::Text(SharedText::from(message)),
            Value::Text(SharedText::from("")),
            Value::Int(BigInt::zero()),
            Value::Int(BigInt::zero()),
        ]),
    ))
}

fn http_response_json_value(
    response: Value,
    artifact: &Artifact,
    budget: &Budget,
    max_text_bytes: usize,
    limits: &VmLimits,
    span: Span,
) -> Result<Value, RuntimeError> {
    let fields = match response {
        Value::Record(name, fields) if name == "HttpResponse" && fields.len() == 3 => fields,
        _ => {
            return Err(RuntimeError::new(
                "R_TYPE_INVALID",
                "http_response_json_bytes argument must be HttpResponse",
                span,
            ));
        }
    };
    let Value::Bytes(body) = &fields[2] else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "HttpResponse body must be Bytes",
            span,
        ));
    };
    if body.len() > max_text_bytes {
        let value = std_error_record_value(
            artifact,
            budget,
            "R_HTTP_LIMIT",
            "HTTP response exceeds the configured text byte limit",
            span,
        )?;
        return Ok(Value::ResultErr(Box::new(value)));
    }
    let text = match std::str::from_utf8(body.as_ref()) {
        Ok(text) => text,
        Err(_) => {
            let value = std_error_record_value(
                artifact,
                budget,
                "R_HTTP_ENCODING",
                "HTTP response body is not valid UTF-8",
                span,
            )?;
            return Ok(Value::ResultErr(Box::new(value)));
        }
    };

    // Match the old Text conversion's guest allocation charge while parsing the
    // response's existing bytes in place, without allocating an intermediate Text.
    charge_allocation(budget, body.len(), span)?;
    let parse_started = budget.runtime_profile.as_ref().map(|_| Instant::now());
    let parsed = parse_unique_shared_json(text);
    if let (Some(profile), Some(started)) = (&budget.runtime_profile, parse_started) {
        profile.record_json_parse(started.elapsed().as_nanos().min(u64::MAX as u128) as u64);
    }
    match parsed {
        Ok(value) => {
            let value = finish_shared_json(value, limits, budget, span)?;
            Ok(Value::ResultOk(Box::new(value)))
        }
        Err(JsonDecodeError::Parse(error)) => {
            let value =
                std_error_record_value(artifact, budget, "R_JSON_PARSE", &error.to_string(), span)?;
            Ok(Value::ResultErr(Box::new(value)))
        }
        Err(JsonDecodeError::DuplicateKey { key, .. }) => {
            let message = format!("JSON object contains duplicate key `{key}`");
            let value =
                std_error_record_value(artifact, budget, "R_JSON_DUPLICATE_KEY", &message, span)?;
            Ok(Value::ResultErr(Box::new(value)))
        }
    }
}

struct HostOperationProfileTimer {
    profile: Arc<RuntimeProfileCounters>,
    operation: &'static str,
    started: Instant,
}

impl Drop for HostOperationProfileTimer {
    fn drop(&mut self) {
        self.profile.record_host_operation(
            self.operation,
            self.started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
        );
    }
}

fn record_native_host_operation(
    profile: Option<&Arc<RuntimeProfileCounters>>,
    started: Option<Instant>,
    operation: &'static str,
) {
    if let (Some(profile), Some(started)) = (profile, started) {
        if profile.should_sample_host_operation() {
            profile.record_host_operation(
                operation,
                started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            );
        }
    }
}

impl VmHost for Instance {
    fn artifact(&self) -> &Arc<Artifact> {
        &self.artifact
    }

    fn register_programs(&self) -> Option<Arc<RegisterPrograms>> {
        Some(Arc::clone(&self.registers))
    }

    fn limits(&self) -> VmLimits {
        self.policy.vm_limits()
    }

    fn externally_cancelled(&self) -> bool {
        self.cancelled
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    }

    fn program_output_bytes(&self) -> usize {
        self.output.len().saturating_add(self.program_stderr.len())
    }

    fn write_program_output(
        &mut self,
        stream: OutputStream,
        text: &str,
        _span: Span,
    ) -> Result<(), RuntimeError> {
        let (destination, kind) = match stream {
            OutputStream::Stdout => (&mut self.output, EffectKind::ProgramOutput),
            OutputStream::Stderr => (&mut self.program_stderr, EffectKind::ProgramErrorOutput),
        };
        destination.push_str(text);
        let effect = self.begin_effect(kind, format!("{} bytes", text.len()));
        self.finish_effect(effect, EffectOutcome::Returned);
        Ok(())
    }

    fn start_frame(
        &mut self,
        index: usize,
        args: Vec<Value>,
        depth: usize,
        budget: &Arc<Budget>,
        call_site: Option<Span>,
    ) -> Result<FrameStart, RuntimeError> {
        let allow_native = self
            .native_code
            .as_ref()
            .is_some_and(|native| native.has_function(index) && !native.vm_call_stays_in_vm(index));
        self.start_guest_frame(index, args, depth, budget.clone(), call_site, allow_native)
    }

    fn host_op(&mut self, context: HostOpContext<'_>) -> Result<(), RuntimeError> {
        let HostOpContext {
            instruction,
            function: _,
            stack,
            locals,
            budget,
            depth,
        } = context;
        let _profile_timer = budget
            .runtime_profile
            .as_ref()
            .filter(|profile| profile.should_sample_host_operation())
            .cloned()
            .map(|profile| HostOperationProfileTimer {
                profile,
                operation: opcode_profile_name(&instruction.op),
                started: Instant::now(),
            });
        macro_rules! push_stack {
            ($value:expr) => {{
                let value = $value;
                if stack.len() == stack.capacity() {
                    let next_capacity = if stack.capacity() == 0 {
                        4
                    } else {
                        stack.capacity().saturating_mul(2)
                    };
                    stack.reserve_exact(next_capacity.saturating_sub(stack.len()));
                }
                stack.push(value);
            }};
        }
        macro_rules! pop {
            () => {
                stack.pop().ok_or_else(|| {
                    RuntimeError::new(
                        "R_STACK_INVALID",
                        "verified bytecode stack underflow",
                        instruction.span,
                    )
                })?
            };
        }
        match instruction.op {
            Op::JsonPushLocal(slot) => {
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_push value must be Json",
                            instruction.span,
                        ));
                    }
                };
                let local = locals.get_mut(slot).ok_or_else(|| {
                    RuntimeError::new(
                        "R_LOCAL_UNINITIALIZED",
                        "json_push assignment targets an uninitialized local",
                        instruction.span,
                    )
                })?;
                let Value::Json(array) = local else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_push assignment target must be Json",
                        instruction.span,
                    ));
                };
                let Some(items) = array.as_array() else {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_push requires a JSON array",
                        instruction.span,
                    ));
                };
                let len = items.len();
                if len >= self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "JSON array exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    shared_json_push_local_allocation(array),
                    instruction.span,
                )?;
                Shared::make_mut(array)
                    .as_array_mut()
                    .expect("array type was checked before copy-on-write")
                    .push(value);
                if let Some(error) = value_limit_error(local, &self.policy, instruction.span) {
                    return Err(error);
                }
            }
            Op::JsonSetLocal(slot) => {
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_set value must be Json",
                            instruction.span,
                        ));
                    }
                };
                let key = match pop!() {
                    Value::Text(key) => key,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_set key must be Text",
                            instruction.span,
                        ));
                    }
                };
                if key.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "JSON key exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                let local = locals.get_mut(slot).ok_or_else(|| {
                    RuntimeError::new(
                        "R_LOCAL_UNINITIALIZED",
                        "json_set assignment targets an uninitialized local",
                        instruction.span,
                    )
                })?;
                let Value::Json(object) = local else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_set assignment target must be Json",
                        instruction.span,
                    ));
                };
                let Some(fields) = object.as_object() else {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_set requires an object",
                        instruction.span,
                    ));
                };
                if !fields.contains_key(key.as_ref())
                    && fields.len() >= self.policy.max_collection_items
                {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "JSON object exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                let shared = Shared::strong_count(object) > 1
                    || matches!(object.as_ref(), SharedJson::Object(values) if Shared::strong_count(values) > 1);
                let copied_bytes = shared_json_set_local_allocation(object, &key);
                charge_allocation(budget.as_ref(), copied_bytes, instruction.span)?;
                Shared::make_mut(object)
                    .as_object_mut()
                    .expect("object type was checked before copy-on-write")
                    .insert(key.to_string(), value);
                if let Some(profile) = &budget.runtime_profile {
                    profile.record_json_set(if shared { copied_bytes } else { 0 });
                }
                if let Some(error) = value_limit_error(local, &self.policy, instruction.span) {
                    return Err(error);
                }
            }
            Op::ParallelMap(target) => {
                let limit = match pop!() {
                    Value::Int(n) => n.to_usize(),
                    _ => None,
                }
                .filter(|n| *n > 0)
                .ok_or_else(|| {
                    RuntimeError::new(
                        "R_PARALLEL_LIMIT",
                        "parallel_map worker limit must be a positive machine-sized integer",
                        instruction.span,
                    )
                })?;
                if limit > self.policy.max_parallelism {
                    return Err(RuntimeError::new(
                        "R_PARALLEL_LIMIT",
                        format!(
                            "worker limit {limit} exceeds host limit {}",
                            self.policy.max_parallelism
                        ),
                        instruction.span,
                    ));
                }
                let values = match pop!() {
                    Value::List(values) => values.as_ref().clone(),
                    Value::Float64List(values) => {
                        values.iter().copied().map(Value::Float64).collect()
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "parallel_map requires a List",
                            instruction.span,
                        ));
                    }
                };
                let input_copy_bytes = size_of::<Vec<Value>>()
                    .saturating_add(values.len().saturating_mul(size_of::<Value>()))
                    .saturating_add(
                        values
                            .iter()
                            .map(cloned_value_allocation)
                            .fold(0usize, usize::saturating_add),
                    );
                charge_allocation(budget.as_ref(), input_copy_bytes, instruction.span)?;
                match self.parallel_map(
                    target,
                    values,
                    limit,
                    depth + 1,
                    budget.clone(),
                    instruction.span,
                ) {
                    Ok(Ok(results)) => {
                        push_stack!(Value::ResultOk(Box::new(Value::from_list_items(results))))
                    }
                    Ok(Err(error)) => {
                        push_stack!(Value::ResultErr(Box::new(Value::TaskError(Box::new(
                            error
                        )))))
                    }
                    Err(mut error) => {
                        error.trace.push(instruction.span);
                        return Err(error);
                    }
                }
            }
            Op::SortBy(target) => {
                let mut values = match pop!() {
                    Value::List(values) => values.as_ref().clone(),
                    Value::Float64List(values) => {
                        values.iter().copied().map(Value::Float64).collect()
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "sort_by requires a List",
                            instruction.span,
                        ));
                    }
                };
                let allocation = size_of::<Vec<Value>>()
                    .saturating_add(values.len().saturating_mul(size_of::<Value>()))
                    .saturating_add(
                        values
                            .iter()
                            .map(cloned_value_allocation)
                            .fold(0usize, usize::saturating_add),
                    );
                charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                if values.len() <= 32 {
                    for index in 1..values.len() {
                        let value = values[index].clone();
                        let mut position = index;
                        while position > 0 {
                            let ordering = match self.invoke_inner(
                                target,
                                vec![value.clone(), values[position - 1].clone()],
                                depth + 1,
                                budget.clone(),
                            ) {
                                Ok(Value::Int(ordering)) => ordering,
                                Ok(_) => {
                                    return Err(RuntimeError::new(
                                        "R_TYPE_INVALID",
                                        "sort comparator must return Int",
                                        instruction.span,
                                    ));
                                }
                                Err(mut error) => {
                                    error.trace.push(instruction.span);
                                    return Err(error);
                                }
                            };
                            if ordering.is_negative() {
                                values[position] = values[position - 1].clone();
                                position -= 1;
                            } else {
                                break;
                            }
                        }
                        values[position] = value;
                    }
                } else {
                    let length = values.len();
                    let index_storage = size_of::<Vec<usize>>()
                        .saturating_mul(2)
                        .saturating_add(size_of::<Vec<bool>>())
                        .saturating_add(length.saturating_mul(size_of::<usize>()).saturating_mul(2))
                        .saturating_add(length);
                    charge_allocation(budget.as_ref(), index_storage, instruction.span)?;

                    let mut order: Vec<usize> = (0..length).collect();
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
                                let take_left = self.sort_by_left_before_or_equal(
                                    target,
                                    &values[order[left]],
                                    &values[order[right]],
                                    depth,
                                    budget.clone(),
                                    instruction.span,
                                )?;
                                if take_left {
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

                    let mut visited = vec![false; length];
                    for start in 0..length {
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
                push_stack!(Value::from_list_items(values));
            }
            Op::SortByIntKey(target) => {
                let mut values = match pop!() {
                    Value::List(values) => values.as_ref().clone(),
                    Value::Float64List(values) => {
                        values.iter().copied().map(Value::Float64).collect()
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "sort_by_int_key requires a List",
                            instruction.span,
                        ));
                    }
                };
                let length = values.len();
                let value_allocation = size_of::<Vec<Value>>()
                    .saturating_add(length.saturating_mul(size_of::<Value>()))
                    .saturating_add(
                        values
                            .iter()
                            .map(cloned_value_allocation)
                            .fold(0usize, usize::saturating_add),
                    );
                let sort_storage = size_of::<Vec<BigInt>>()
                    .saturating_add(size_of::<Vec<usize>>().saturating_mul(2))
                    .saturating_add(size_of::<Vec<bool>>())
                    .saturating_add(
                        length.saturating_mul(
                            size_of::<BigInt>()
                                .saturating_add(size_of::<usize>().saturating_mul(2))
                                .saturating_add(size_of::<bool>()),
                        ),
                    );
                charge_allocation(
                    budget.as_ref(),
                    value_allocation.saturating_add(sort_storage),
                    instruction.span,
                )?;

                let mut keys = Vec::with_capacity(length);
                for value in &values {
                    match self.invoke_inner(target, vec![value.clone()], depth + 1, budget.clone())
                    {
                        Ok(Value::Int(key)) => keys.push(key),
                        Ok(_) => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "sort_by_int_key key function must return Int",
                                instruction.span,
                            ));
                        }
                        Err(mut error) => {
                            error.trace.push(instruction.span);
                            return Err(error);
                        }
                    }
                }

                let mut order: Vec<usize> = (0..length).collect();
                if length <= 32 {
                    for index in 1..length {
                        let value = order[index];
                        let mut position = index;
                        while position > 0 {
                            consume_sort_step(budget.as_ref(), instruction.span)?;
                            if keys[value] < keys[order[position - 1]] {
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
                                consume_sort_step(budget.as_ref(), instruction.span)?;
                                if keys[order[left]] <= keys[order[right]] {
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

                let mut visited = vec![false; length];
                for start in 0..length {
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
                push_stack!(Value::from_list_items(values));
            }
            Op::MergeSortedByIntKey(target) => {
                let input_feeds = match pop!() {
                    Value::List(feeds) => feeds.as_ref().clone(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "merge_sorted_by_int_key requires a List of Lists",
                            instruction.span,
                        ));
                    }
                };
                let mut feeds = Vec::with_capacity(input_feeds.len());
                let mut total = 0usize;
                for feed in input_feeds {
                    let values: Shared<Vec<Value>> = match feed {
                        Value::List(values) => values,
                        Value::Float64List(values) => {
                            Shared::new(values.iter().copied().map(Value::Float64).collect())
                        }
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "merge_sorted_by_int_key requires each feed to be a List",
                                instruction.span,
                            ));
                        }
                    };
                    total = total.saturating_add(values.len());
                    feeds.push(values);
                }
                if total > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "merged list exceeds the configured collection limit",
                        instruction.span,
                    ));
                }
                let value_allocation = size_of::<Vec<Value>>()
                    .saturating_add(total.saturating_mul(size_of::<Value>()))
                    .saturating_add(
                        feeds
                            .iter()
                            .flat_map(|feed| feed.iter())
                            .map(cloned_value_allocation)
                            .fold(0usize, usize::saturating_add),
                    );
                let key_storage = size_of::<Vec<Shared<Vec<Value>>>>()
                    .saturating_add(feeds.len().saturating_mul(size_of::<Shared<Vec<Value>>>()))
                    .saturating_add(size_of::<Vec<Vec<BigInt>>>())
                    .saturating_add(feeds.len().saturating_mul(size_of::<Vec<BigInt>>()))
                    .saturating_add(total.saturating_mul(size_of::<BigInt>()))
                    .saturating_add(size_of::<Vec<usize>>())
                    .saturating_add(feeds.len().saturating_mul(size_of::<usize>()));
                charge_allocation(
                    budget.as_ref(),
                    value_allocation.saturating_add(key_storage),
                    instruction.span,
                )?;

                let mut keys = Vec::with_capacity(feeds.len());
                for feed in &feeds {
                    let mut feed_keys = Vec::with_capacity(feed.len());
                    for value in feed.iter() {
                        match self.invoke_inner(
                            target,
                            vec![value.clone()],
                            depth + 1,
                            budget.clone(),
                        ) {
                            Ok(Value::Int(key)) => feed_keys.push(key),
                            Ok(_) => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "merge_sorted_by_int_key key function must return Int",
                                    instruction.span,
                                ));
                            }
                            Err(mut error) => {
                                error.trace.push(instruction.span);
                                return Err(error);
                            }
                        }
                    }
                    for index in 1..feed_keys.len() {
                        consume_sort_step(budget.as_ref(), instruction.span)?;
                        if feed_keys[index] < feed_keys[index - 1] {
                            return Err(RuntimeError::new(
                                "R_SORT_ORDER",
                                "merge_sorted_by_int_key feeds must already be sorted by key",
                                instruction.span,
                            ));
                        }
                    }
                    keys.push(feed_keys);
                }

                let mut positions = vec![0usize; feeds.len()];
                let mut output = Vec::with_capacity(total);
                while output.len() < total {
                    let mut selected: Option<usize> = None;
                    for feed_index in 0..feeds.len() {
                        let position = positions[feed_index];
                        if position >= feeds[feed_index].len() {
                            continue;
                        }
                        if let Some(best) = selected {
                            consume_sort_step(budget.as_ref(), instruction.span)?;
                            if keys[feed_index][position] < keys[best][positions[best]] {
                                selected = Some(feed_index);
                            }
                        } else {
                            selected = Some(feed_index);
                        }
                    }
                    let feed_index = selected.expect("remaining merge item must have a feed");
                    let position = positions[feed_index];
                    output.push(feeds[feed_index][position].clone());
                    positions[feed_index] = position + 1;
                }
                push_stack!(Value::from_list_items(output));
            }
            Op::HttpGet => {
                let url = match pop!() {
                    Value::Text(url) => url,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "http_get URL must be Text",
                            instruction.span,
                        ));
                    }
                };
                let http = self.shared_http_client(instruction.span)?;
                let effect =
                    self.begin_effect(EffectKind::HttpRequest, http_effect_target("GET", &url));
                match perform_http_get(&http, &self.policy, &url, budget.as_ref()) {
                    Ok(text) => {
                        self.finish_effect(effect, EffectOutcome::Returned);
                        charge_allocation(
                            budget.as_ref(),
                            size_of::<String>().saturating_add(text.len()),
                            instruction.span,
                        )?;
                        push_stack!(Value::ResultOk(Box::new(Value::Text(text.into()))));
                    }
                    Err(mut error) => {
                        if budget.is_cancelled() {
                            self.finish_effect(effect, EffectOutcome::Interrupted);
                            return Ok(());
                        }
                        self.finish_effect(effect, EffectOutcome::ReturnedError);
                        error.trace.push(instruction.span);
                        push_stack!(Value::ResultErr(Box::new(Value::HttpError(Box::new(
                            error
                        )))));
                    }
                }
            }
            Op::HttpRequest | Op::HttpRequestWithTimeout | Op::HttpRequestBytes => {
                let byte_response = matches!(&instruction.op, Op::HttpRequestBytes);
                let has_timeout = matches!(
                    &instruction.op,
                    Op::HttpRequestWithTimeout | Op::HttpRequestBytes
                );
                let request_timeout = if has_timeout {
                    match pop!() {
                        Value::Int(milliseconds) => match milliseconds
                            .to_usize()
                            .and_then(|value| u64::try_from(value).ok())
                        {
                            Some(value) if value > 0 => {
                                self.policy.http_timeout.min(Duration::from_millis(value))
                            }
                            _ => {
                                let mut error = http_error(
                                    "R_HTTP_TIMEOUT",
                                    "per-request timeout_ms must be a positive 64-bit integer",
                                );
                                error.trace.push(instruction.span);
                                push_stack!(Value::ResultErr(Box::new(Value::HttpError(
                                    Box::new(error)
                                ))));
                                return Ok(());
                            }
                        },
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "HTTP timeout_ms must be Int",
                                instruction.span,
                            ));
                        }
                    }
                } else {
                    self.policy.http_timeout
                };
                let body = match pop!() {
                    Value::Text(body) if !byte_response => body.as_bytes().to_vec(),
                    Value::Bytes(body) if byte_response => body.as_ref().clone(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            if byte_response {
                                "http_request_bytes body must be Bytes"
                            } else {
                                "http_request body must be Text"
                            },
                            instruction.span,
                        ));
                    }
                };
                let headers = match pop!() {
                    Value::Map(headers) => {
                        let mut decoded = BTreeMap::new();
                        for (name, value) in headers.iter() {
                            let Value::Text(value) = value else {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "http_request headers must contain Text values",
                                    instruction.span,
                                ));
                            };
                            decoded.insert(name.to_string(), value.to_string());
                        }
                        decoded
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "http_request headers must be Map[Text]",
                            instruction.span,
                        ));
                    }
                };
                let url = match pop!() {
                    Value::Text(url) => url,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "http_request URL must be Text",
                            instruction.span,
                        ));
                    }
                };
                let method = match pop!() {
                    Value::Text(method) => method,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "http_request method must be Text",
                            instruction.span,
                        ));
                    }
                };
                let http = self.shared_http_client(instruction.span)?;
                let effect =
                    self.begin_effect(EffectKind::HttpRequest, http_effect_target(&method, &url));
                match perform_http_request_bytes(
                    &http,
                    &self.policy,
                    HttpRequestBytes {
                        method: &method,
                        input: &url,
                        headers: &headers,
                        body: &body,
                        timeout: request_timeout,
                    },
                    budget.as_ref(),
                ) {
                    Ok(response) => {
                        if byte_response {
                            self.finish_effect(effect, EffectOutcome::Returned);
                            let size = response.body.len().saturating_add(
                                response
                                    .headers
                                    .iter()
                                    .map(|(k, v)| k.len() + v.len())
                                    .sum::<usize>(),
                            );
                            charge_allocation(budget.as_ref(), size, instruction.span)?;
                            let response_headers = response
                                .headers
                                .into_iter()
                                .map(|(key, value)| (key, Value::Text(value.into())))
                                .collect();
                            push_stack!(Value::ResultOk(Box::new(Value::Record(
                                "HttpResponse".into(),
                                Shared::new(vec![
                                    Value::Int(BigInt::from_i64(response.status as i64)),
                                    Value::Map(Shared::new(response_headers)),
                                    Value::Bytes(Shared::new(response.body)),
                                ]),
                            ))));
                        } else {
                            match response.into_text_json(self.policy.max_text_bytes) {
                                Ok(response) => {
                                    self.finish_effect(effect, EffectOutcome::Returned);
                                    let value = finish_json_value(
                                        response,
                                        &self.policy.vm_limits(),
                                        budget.as_ref(),
                                        instruction.span,
                                    )?;
                                    push_stack!(Value::ResultOk(Box::new(value)));
                                }
                                Err(mut error) => {
                                    self.finish_effect(effect, EffectOutcome::ReturnedError);
                                    error.trace.push(instruction.span);
                                    push_stack!(Value::ResultErr(Box::new(Value::HttpError(
                                        Box::new(error)
                                    ))));
                                }
                            }
                        }
                    }
                    Err(mut error) => {
                        if budget.is_cancelled() {
                            self.finish_effect(effect, EffectOutcome::Interrupted);
                            return Ok(());
                        }
                        self.finish_effect(effect, EffectOutcome::ReturnedError);
                        error.trace.push(instruction.span);
                        push_stack!(Value::ResultErr(Box::new(Value::HttpError(Box::new(
                            error
                        )))));
                    }
                }
            }
            Op::HttpResponseText => {
                let response = pop!();
                push_stack!(http_response_text_value(
                    response,
                    budget.as_ref(),
                    self.policy.max_text_bytes,
                    instruction.span,
                )?);
            }
            Op::HttpResponseJson => {
                let response = pop!();
                push_stack!(http_response_json_value(
                    response,
                    &self.artifact,
                    budget.as_ref(),
                    self.policy.max_text_bytes,
                    &self.policy.vm_limits(),
                    instruction.span,
                )?);
            }
            Op::FileOpenLines | Op::CsvOpen => {
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "file reader path must be Text",
                            instruction.span,
                        ));
                    }
                };
                let resolved = match host_file_path(&self.policy, &path, false, instruction.span) {
                    Ok(path) => path,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                let effect = self.begin_effect(EffectKind::FileRead, path.as_ref());
                let opened = if instruction.op == Op::FileOpenLines {
                    self.readers.open_lines(resolved)
                } else {
                    self.readers.open_csv(resolved)
                };
                match opened {
                    Ok(handle) => {
                        self.finish_effect(effect, EffectOutcome::Returned);
                        let (owner, id) = handle.parts();
                        push_stack!(Value::ResultOk(Box::new(Value::NativeResource(Box::new(
                            NativeResourceValue::new("FileReader", owner, id),
                        )))));
                    }
                    Err(error) => {
                        self.finish_effect(effect, EffectOutcome::ReturnedError);
                        let code = match error.kind {
                            stdlib_data::ReaderErrorKind::InvalidUtf8 => "R_FILE_UTF8",
                            stdlib_data::ReaderErrorKind::Limit => "R_FILE_LIMIT",
                            stdlib_data::ReaderErrorKind::ForeignOwner => "R_RESOURCE_OWNER",
                            stdlib_data::ReaderErrorKind::Closed => "R_RESOURCE_CLOSED",
                            stdlib_data::ReaderErrorKind::Stale => "R_RESOURCE_STALE",
                            stdlib_data::ReaderErrorKind::Mode => "R_RESOURCE_MODE",
                            stdlib_data::ReaderErrorKind::Io => "R_FILE_READ",
                        };
                        push_stack!(file_error_result(RuntimeError::new(
                            code,
                            error.message,
                            instruction.span
                        )));
                    }
                }
            }
            Op::FileNextLine | Op::CsvNext | Op::FileClose => {
                let value = pop!();
                let handle = runtime_reader_handle(&value, instruction.span)?;
                if instruction.op == Op::FileClose {
                    match self.readers.close(handle) {
                        Ok(()) => push_stack!(Value::ResultOk(Box::new(Value::Unit))),
                        Err(error) => {
                            let error = reader_runtime_error(
                                error.kind,
                                error.message,
                                error.line,
                                error.column,
                                instruction.span,
                            );
                            if error.code == "R_FILE_LIMIT" {
                                return Err(error);
                            }
                            push_stack!(file_error_result(error));
                        }
                    }
                } else if instruction.op == Op::FileNextLine {
                    match self.readers.next_line(handle) {
                        Ok(Some(line)) => push_stack!(Value::ResultOk(Box::new(
                            Value::OptionSome(Box::new(Value::Record(
                                source_line_record_name(),
                                Shared::new(vec![
                                    Value::Int(BigInt::from_usize(line.number)),
                                    Value::Text(line.text)
                                ]),
                            )))
                        ))),
                        Ok(None) => push_stack!(Value::ResultOk(Box::new(Value::OptionNone))),
                        Err(error) => push_stack!(file_error_result(reader_runtime_error(
                            error.kind,
                            error.message,
                            error.line,
                            error.column,
                            instruction.span
                        ))),
                    }
                } else {
                    match self.readers.next_csv(handle) {
                        Ok(Some(row)) => {
                            let within_limits = self.policy.max_integer_digits >= 19
                                && row.fields.len() <= self.policy.max_collection_items
                                && row
                                    .fields
                                    .iter()
                                    .all(|field| field.len() <= self.policy.max_text_bytes);
                            let result = Value::ResultOk(Box::new(Value::OptionSome(Box::new(
                                Value::Record(
                                    csv_row_record_name(),
                                    Shared::new(vec![
                                        Value::Int(BigInt::from_usize(row.start_line)),
                                        Value::from_list_items(
                                            row.fields.into_iter().map(Value::Text).collect(),
                                        ),
                                    ]),
                                ),
                            ))));
                            if !within_limits {
                                if let Some(error) =
                                    value_limit_error(&result, &self.policy, instruction.span)
                                {
                                    return Err(error);
                                }
                            }
                            push_stack!(result);
                        }
                        Ok(None) => push_stack!(Value::ResultOk(Box::new(Value::OptionNone))),
                        Err(error) => {
                            let (code, line, column, message) = match error {
                                stdlib_data::CsvError::Reader(error) => {
                                    let error = reader_runtime_error(
                                        error.kind,
                                        error.message,
                                        error.line,
                                        error.column,
                                        instruction.span,
                                    );
                                    if error.code == "R_FILE_LIMIT" {
                                        return Err(error);
                                    }
                                    push_stack!(file_error_result(error));
                                    return Ok(());
                                }
                                stdlib_data::CsvError::Mode => (
                                    "R_RESOURCE_MODE",
                                    0,
                                    0,
                                    "CSV operation requires a CSV reader".into(),
                                ),
                                stdlib_data::CsvError::Malformed {
                                    line,
                                    column,
                                    message,
                                } => ("R_CSV_MALFORMED", line, column, message),
                            };
                            let mut error = RuntimeError::new(code, message, instruction.span);
                            if line > 0 {
                                error.trace.push(Span {
                                    line,
                                    column,
                                    ..instruction.span
                                });
                            }
                            push_stack!(file_error_result(error));
                        }
                    }
                }
            }
            Op::CsvReadColumns => {
                let kinds = pop!();
                let path = pop!();
                let (Value::Text(path), Value::Map(kinds)) = (path, kinds) else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "csv_read_columns requires a Text path and Map[Text, Text] kinds",
                        instruction.span,
                    ));
                };
                let mut fixed = std::collections::BTreeMap::new();
                for (name, kind) in kinds.iter() {
                    let Value::Text(kind) = kind else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "csv_read_columns kinds must be Map[Text, Text]",
                            instruction.span,
                        ));
                    };
                    fixed.insert(name.to_string(), kind.to_string());
                }
                let resolved = match host_file_path(&self.policy, &path, false, instruction.span) {
                    Ok(path) => path,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                let effect = self.begin_effect(EffectKind::FileRead, path.as_ref());
                let read = {
                    let budget = budget.as_ref();
                    let mut charge = |bytes: usize| budget.allocate(bytes);
                    let mut release = |bytes: usize| budget.refund_allocation(bytes);
                    let cancelled = || budget.is_cancelled();
                    let mut meter = stdlib_data::csv_columns::Meter {
                        charge: &mut charge,
                        release: &mut release,
                        cancelled: &cancelled,
                    };
                    stdlib_data::csv_columns::read_csv_columns(
                        &resolved,
                        &fixed,
                        self.policy.max_file_bytes,
                        &mut meter,
                    )
                };
                use stdlib_data::csv_columns::ColumnsError;
                match read {
                    Ok(columns) => {
                        // The names and records the result holds, beyond the arrays and
                        // dictionary text that were charged as they grew.
                        let produced = stdlib_data::csv_columns::result_overhead(&columns);
                        if let Err(error) =
                            charge_allocation(budget.as_ref(), produced, instruction.span)
                        {
                            self.finish_effect(effect, EffectOutcome::ReturnedError);
                            return Err(error);
                        }
                        let result = Value::ResultOk(Box::new(Value::from_list_items(
                            columns.into_iter().map(csv_column_value).collect(),
                        )));
                        if let Some(error) =
                            value_limit_error(&result, &self.policy, instruction.span)
                        {
                            self.finish_effect(effect, EffectOutcome::ReturnedError);
                            return Err(error);
                        }
                        self.finish_effect(effect, EffectOutcome::Returned);
                        push_stack!(result);
                    }
                    Err(ColumnsError::Allocation(bytes)) => {
                        self.finish_effect(effect, EffectOutcome::ReturnedError);
                        return Err(guion_vm_core::allocation_limit_error(
                            budget.as_ref(),
                            bytes,
                            instruction.span,
                        ));
                    }
                    Err(ColumnsError::Cancelled) => {
                        self.finish_effect(effect, EffectOutcome::Interrupted);
                        return Ok(());
                    }
                    Err(error) => {
                        self.finish_effect(effect, EffectOutcome::ReturnedError);
                        let (code, line, column, message) = match error {
                            ColumnsError::Reader(error) => {
                                let error = reader_runtime_error(
                                    error.kind,
                                    error.message,
                                    error.line,
                                    error.column,
                                    instruction.span,
                                );
                                if error.code == "R_FILE_LIMIT" {
                                    return Err(error);
                                }
                                push_stack!(file_error_result(error));
                                return Ok(());
                            }
                            ColumnsError::Malformed {
                                line,
                                column,
                                message,
                            } => ("R_CSV_MALFORMED", line, column, message),
                            ColumnsError::Value {
                                line,
                                column,
                                message,
                            } => ("E_CSV_VALUE", line, column, message),
                            ColumnsError::Allocation(_) | ColumnsError::Cancelled => {
                                unreachable!("handled above")
                            }
                        };
                        // The error's first position is the place in the file, which is what
                        // `file_error_line` and `file_error_column` report.
                        let mut error = RuntimeError::new(code, message, instruction.span);
                        if line > 0 {
                            error.trace = vec![Span {
                                line,
                                column,
                                ..instruction.span
                            }];
                        }
                        push_stack!(file_error_result(error));
                    }
                }
            }
            Op::CsvEncode => {
                let rows = match pop!() {
                    Value::List(rows) => rows,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "csv_encode requires List[List[Text]]",
                            instruction.span,
                        ));
                    }
                };
                let mut encoded_rows = Vec::with_capacity(rows.len());
                for row in rows.iter() {
                    let Value::List(fields) = row else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "csv_encode row requires List[Text]",
                            instruction.span,
                        ));
                    };
                    let fields = fields
                        .iter()
                        .map(|value| match value {
                            Value::Text(text) => Ok(text.to_string()),
                            _ => Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "csv_encode field requires Text",
                                instruction.span,
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    encoded_rows.push(fields);
                }
                let text = stdlib_data::csv_encode(&encoded_rows);
                if text.len() > self.policy.max_file_bytes
                    || text.len() > self.policy.max_text_bytes
                {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "CSV output exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    size_of::<String>().saturating_add(text.len()),
                    instruction.span,
                )?;
                push_stack!(Value::ResultOk(Box::new(Value::Text(text.into()))));
            }
            Op::TimeParseDate | Op::TimeParseTimestamp | Op::SemVerParse => {
                let text = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "data parser requires Text",
                            instruction.span,
                        ));
                    }
                };
                if text.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "data parser input exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                let parsed: Result<Value, stdlib_data::DataError> = match instruction.op {
                    Op::TimeParseDate => stdlib_data::Date::parse(&text).map(|date| {
                        Value::Record(
                            "Date".into(),
                            Shared::new(vec![
                                Value::Int(BigInt::from_i64(date.year as i64)),
                                Value::Int(BigInt::from_u64(date.month as u64)),
                                Value::Int(BigInt::from_u64(date.day as u64)),
                            ]),
                        )
                    }),
                    Op::TimeParseTimestamp => stdlib_data::parse_timestamp(&text).map(|micros| {
                        Value::Record(
                            "Instant".into(),
                            Shared::new(vec![Value::Int(BigInt::from_i64(micros))]),
                        )
                    }),
                    Op::SemVerParse => stdlib_data::parse_semver(&text).map(|version| {
                        Value::Record(
                            "SemVer".into(),
                            Shared::new(vec![
                                Value::Text(version.major.into()),
                                Value::Text(version.minor.into()),
                                Value::Text(version.patch.into()),
                                Value::from_list_items(
                                    version
                                        .prerelease
                                        .into_iter()
                                        .map(|value| Value::Text(value.into()))
                                        .collect(),
                                ),
                                Value::from_list_items(
                                    version
                                        .build
                                        .into_iter()
                                        .map(|value| Value::Text(value.into()))
                                        .collect(),
                                ),
                                Value::Text(version.original.into()),
                            ]),
                        )
                    }),
                    _ => unreachable!(),
                };
                match parsed {
                    Ok(value) => push_stack!(Value::ResultOk(Box::new(value))),
                    Err(error) => push_stack!(file_error_result(RuntimeError::new(
                        error.code,
                        error.message,
                        instruction.span
                    ))),
                }
            }
            Op::TimeResolveLocal => {
                let timezone = match pop!() {
                    Value::Text(timezone) => timezone,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "resolve_local timezone must be Text",
                            instruction.span,
                        ));
                    }
                };
                let fields = match pop!() {
                    Value::Record(name, fields) if name == "LocalDateTime" && fields.len() == 7 => {
                        fields
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "resolve_local requires a LocalDateTime record",
                            instruction.span,
                        ));
                    }
                };
                let integer_field = |index: usize, field: &str, code: &str| match &fields[index] {
                    Value::Int(value) => value.to_i64().ok_or_else(|| {
                        stdlib_services::ServiceError::new(
                            code,
                            format!("{field} is outside the supported integer range"),
                        )
                    }),
                    _ => Err(stdlib_services::ServiceError::new(
                        "E_TIME_INPUT",
                        format!("LocalDateTime.{field} is not an integer"),
                    )),
                };
                let local = (|| {
                    Ok(stdlib_services::LocalDateTime {
                        year: integer_field(0, "year", "E_DATE")?,
                        month: integer_field(1, "month", "E_DATE")?,
                        day: integer_field(2, "day", "E_DATE")?,
                        hour: integer_field(3, "hour", "E_TIME")?,
                        minute: integer_field(4, "minute", "E_TIME")?,
                        second: integer_field(5, "second", "E_TIME")?,
                        microsecond: integer_field(6, "microsecond", "E_TIME")?,
                    })
                })();
                let result =
                    local.and_then(|local| stdlib_services::resolve_local_time(local, &timezone));
                let value = temporal_resolution_result_value(result);
                charge_allocation(
                    budget.as_ref(),
                    cloned_value_allocation(&value),
                    instruction.span,
                )?;
                push_stack!(value);
            }
            Op::TimeZoneDataVersion => {
                let version = Value::Text(stdlib_services::IANA_TZDB_VERSION.to_owned().into());
                charge_allocation(
                    budget.as_ref(),
                    stdlib_services::IANA_TZDB_VERSION.len(),
                    instruction.span,
                )?;
                push_stack!(version);
            }
            Op::TimeFormatUtc => {
                let value = pop!();
                let Value::Record(name, fields) = value else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "time_format_utc requires Instant",
                        instruction.span,
                    ));
                };
                if name != "Instant" || fields.len() != 1 {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "invalid Instant record",
                        instruction.span,
                    ));
                }
                let Value::Int(micros) = &fields[0] else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "invalid Instant value",
                        instruction.span,
                    ));
                };
                let micros = micros.to_i64().ok_or_else(|| {
                    RuntimeError::new(
                        "R_TIME_RANGE",
                        "instant is outside supported range",
                        instruction.span,
                    )
                })?;
                let text = stdlib_data::format_utc(stdlib_data::Instant {
                    unix_microseconds: micros,
                });
                if text.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "formatted timestamp exceeds configured text limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    size_of::<String>().saturating_add(text.len()),
                    instruction.span,
                )?;
                push_stack!(Value::Text(text.into()));
            }
            Op::TimeFormatUtcDate => {
                let value = pop!();
                let Value::Record(name, fields) = value else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "time_format_utc_date requires Instant",
                        instruction.span,
                    ));
                };
                if name != "Instant" || fields.len() != 1 {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "invalid Instant record",
                        instruction.span,
                    ));
                }
                let Value::Int(micros) = &fields[0] else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "invalid Instant value",
                        instruction.span,
                    ));
                };
                let micros = micros.to_i64().ok_or_else(|| {
                    RuntimeError::new(
                        "R_TIME_RANGE",
                        "instant is outside supported range",
                        instruction.span,
                    )
                })?;
                let text = stdlib_data::format_utc_date(stdlib_data::Instant {
                    unix_microseconds: micros,
                });
                if text.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "formatted UTC date exceeds configured text limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    size_of::<String>().saturating_add(text.len()),
                    instruction.span,
                )?;
                push_stack!(Value::Text(text.into()));
            }
            Op::UnixTimeMs | Op::TimeTodayLocal => {
                if !self.policy.allow_clock {
                    return Err(RuntimeError::new(
                        "R_CLOCK_DENIED",
                        "wall-clock access is not granted",
                        instruction.span,
                    ));
                }
                let effect = self.begin_effect(EffectKind::ClockRead, "system-clock");
                let unix_time_ms = match self.policy.clock_unix_time_ms_override {
                    Some(value) => value,
                    None => match current_unix_time_ms() {
                        Ok(value) => value,
                        Err(message) => {
                            self.finish_effect(effect, EffectOutcome::ReturnedError);
                            return Err(RuntimeError::new(
                                "R_CLOCK_UNAVAILABLE",
                                message,
                                instruction.span,
                            ));
                        }
                    },
                };
                if matches!(instruction.op, Op::UnixTimeMs) {
                    self.finish_effect(effect, EffectOutcome::Returned);
                    push_stack!(Value::Int(BigInt::from_i64(unix_time_ms)));
                } else {
                    match local_date_from_unix_time_ms(unix_time_ms) {
                        Ok((year, month, day)) => {
                            self.finish_effect(effect, EffectOutcome::Returned);
                            push_stack!(Value::Record(
                                "Date".into(),
                                Shared::new(vec![
                                    Value::Int(BigInt::from_i64(year as i64)),
                                    Value::Int(BigInt::from_i64(month as i64)),
                                    Value::Int(BigInt::from_i64(day as i64)),
                                ]),
                            ));
                        }
                        Err(message) => {
                            self.finish_effect(effect, EffectOutcome::ReturnedError);
                            return Err(RuntimeError::new(
                                "R_CLOCK_RANGE",
                                message,
                                instruction.span,
                            ));
                        }
                    }
                }
            }
            Op::SemVerCompare => {
                let right = pop!();
                let left = pop!();
                let parse_record = |value: &Value| -> Result<stdlib_data::SemVer, RuntimeError> {
                    let Value::Record(name, fields) = value else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "semver_compare requires SemVer records",
                            instruction.span,
                        ));
                    };
                    if name != "SemVer" || fields.len() != 6 {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "invalid SemVer record",
                            instruction.span,
                        ));
                    }
                    let Value::Text(original) = &fields[5] else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "invalid SemVer source text",
                            instruction.span,
                        ));
                    };
                    stdlib_data::parse_semver(original).map_err(|error| {
                        RuntimeError::new("R_TYPE_INVALID", error.message, instruction.span)
                    })
                };
                let left = parse_record(&left)?;
                let right = parse_record(&right)?;
                let ordering = stdlib_data::compare_semver(&left, &right);
                push_stack!(Value::Int(BigInt::from_i64(match ordering {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                })));
            }
            Op::CliParse => {
                let options = match pop!() {
                    Value::List(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "cli_parse options must be a list",
                            instruction.span,
                        ));
                    }
                };
                let arguments = match pop!() {
                    Value::List(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "cli_parse arguments must be a list",
                            instruction.span,
                        ));
                    }
                };
                let arguments = arguments
                    .iter()
                    .map(|value| match value {
                        Value::Text(value) => Ok(value.to_string()),
                        _ => Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "cli_parse argument must be Text",
                            instruction.span,
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let mut parsed_options = Vec::with_capacity(options.len());
                for option in options.iter() {
                    let Value::Record(name, fields) = option else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "cli_parse option must be CliOption",
                            instruction.span,
                        ));
                    };
                    if name != "CliOption" || fields.len() != 4 {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "invalid CliOption record",
                            instruction.span,
                        ));
                    }
                    let (
                        Value::Text(name),
                        Value::Bool(takes_value),
                        Value::Bool(repeatable),
                        Value::Bool(required),
                    ) = (&fields[0], &fields[1], &fields[2], &fields[3])
                    else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "invalid CliOption fields",
                            instruction.span,
                        ));
                    };
                    parsed_options.push(stdlib_data::CliOption::new(
                        name.to_string(),
                        *takes_value,
                        *repeatable,
                        *required,
                    ));
                }
                match stdlib_data::CliArguments::parse(&arguments, &parsed_options) {
                    Ok(parsed) => {
                        let values = parsed
                            .values
                            .into_iter()
                            .map(|(key, values)| {
                                (
                                    key,
                                    Value::from_list_items(
                                        values
                                            .into_iter()
                                            .map(|value| Value::Text(value.into()))
                                            .collect(),
                                    ),
                                )
                            })
                            .collect::<TextMap>();
                        let result = Value::Record(
                            "CliArgs".into(),
                            Shared::new(vec![
                                Value::Map(Shared::new(values)),
                                Value::from_list_items(
                                    parsed
                                        .positionals
                                        .into_iter()
                                        .map(|value| Value::Text(value.into()))
                                        .collect(),
                                ),
                            ]),
                        );
                        push_stack!(Value::ResultOk(Box::new(result)));
                    }
                    Err(error) => push_stack!(file_error_result(RuntimeError::new(
                        error.code,
                        error.message,
                        instruction.span
                    ))),
                }
            }
            Op::Fs(function) => {
                let mut operands = Vec::with_capacity(function.arity());
                for _ in 0..function.arity() {
                    match pop!() {
                        Value::Text(text) => operands.push(text.as_ref().to_owned()),
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "filesystem paths must be Text",
                                instruction.span,
                            ));
                        }
                    }
                }
                operands.reverse();
                let effect = self.begin_effect(
                    if function.writes() {
                        EffectKind::FileWrite
                    } else {
                        EffectKind::FileRead
                    },
                    operands[0].as_str(),
                );
                let outcome = fs_ops::run(&self.policy, function, &operands, instruction.span);
                self.finish_effect(
                    effect,
                    match &outcome {
                        Ok(Value::ResultErr(_)) => EffectOutcome::ReturnedError,
                        _ => EffectOutcome::Returned,
                    },
                );
                let value = outcome?;
                charge_allocation(
                    budget.as_ref(),
                    guion_vm_core::cloned_value_allocation(&value),
                    instruction.span,
                )?;
                push_stack!(value);
            }
            Op::Tls(function) => {
                let mut operands = Vec::with_capacity(function.arity());
                for _ in 0..function.arity() {
                    operands.push(pop!());
                }
                operands.reverse();
                let target = match (function, operands.first()) {
                    (
                        guion_model::TlsFunction::Connect
                        | guion_model::TlsFunction::ConnectIdentity,
                        Some(Value::Text(host)),
                    ) => host.as_ref().to_owned(),
                    _ => function.op_name().to_owned(),
                };
                let effect = self.begin_effect(EffectKind::NetworkSocket, target.as_str());
                let outcome = net_ops::run_tls(
                    &self.policy,
                    &mut self.sockets,
                    function,
                    operands,
                    instruction.span,
                );
                self.finish_effect(
                    effect,
                    match &outcome {
                        Ok(Value::ResultErr(_)) => EffectOutcome::ReturnedError,
                        _ => EffectOutcome::Returned,
                    },
                );
                let value = outcome?;
                charge_allocation(
                    budget.as_ref(),
                    guion_vm_core::cloned_value_allocation(&value),
                    instruction.span,
                )?;
                push_stack!(value);
            }
            Op::Net(function) => {
                let mut operands = Vec::with_capacity(function.arity());
                for _ in 0..function.arity() {
                    operands.push(pop!());
                }
                operands.reverse();
                let target = match operands.first() {
                    Some(Value::Text(host)) => host.as_ref().to_owned(),
                    _ => function.op_name().to_owned(),
                };
                let effect = self.begin_effect(EffectKind::NetworkSocket, target.as_str());
                let outcome = net_ops::run(
                    &self.policy,
                    &mut self.sockets,
                    function,
                    operands,
                    instruction.span,
                );
                self.finish_effect(
                    effect,
                    match &outcome {
                        Ok(Value::ResultErr(_)) => EffectOutcome::ReturnedError,
                        _ => EffectOutcome::Returned,
                    },
                );
                let value = outcome?;
                charge_allocation(
                    budget.as_ref(),
                    guion_vm_core::cloned_value_allocation(&value),
                    instruction.span,
                )?;
                push_stack!(value);
            }
            Op::Process(function) => {
                let value = match function {
                    guion_model::ProcessFunction::Run => {
                        let stdin = match pop!() {
                            Value::Text(text) => text.as_ref().to_owned(),
                            _ => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "process_run standard input must be Text",
                                    instruction.span,
                                ))
                            }
                        };
                        let arguments = match pop!() {
                            Value::List(items) => {
                                let mut arguments = Vec::with_capacity(items.len());
                                for item in items.iter() {
                                    let Value::Text(text) = item else {
                                        return Err(RuntimeError::new(
                                            "R_TYPE_INVALID",
                                            "process_run arguments must be Text",
                                            instruction.span,
                                        ));
                                    };
                                    arguments.push(text.as_ref().to_owned());
                                }
                                arguments
                            }
                            _ => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "process_run arguments must be a list",
                                    instruction.span,
                                ))
                            }
                        };
                        let command = match pop!() {
                            Value::Text(text) => text.as_ref().to_owned(),
                            _ => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "process_run command must be Text",
                                    instruction.span,
                                ))
                            }
                        };
                        let effect = self.begin_effect(EffectKind::ProcessSpawn, command.as_str());
                        let outcome = process_ops::run(&self.policy, &command, &arguments, &stdin);
                        self.finish_effect(
                            effect,
                            match &outcome {
                                Value::ResultErr(_) => EffectOutcome::ReturnedError,
                                _ => EffectOutcome::Returned,
                            },
                        );
                        outcome
                    }
                    guion_model::ProcessFunction::Pid => {
                        Value::Int(BigInt::from_i64(i64::from(std::process::id())))
                    }
                    guion_model::ProcessFunction::SignalPoll => {
                        Value::Text(SharedText::from(process_ops::signal_poll()))
                    }
                };
                charge_allocation(
                    budget.as_ref(),
                    guion_vm_core::cloned_value_allocation(&value),
                    instruction.span,
                )?;
                push_stack!(value);
            }
            Op::FsWalk => {
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "fs_walk path must be Text",
                            instruction.span,
                        ));
                    }
                };
                let resolved = match host_file_path_no_follow(&self.policy, &path, instruction.span)
                {
                    Ok(path) => path,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                match stdlib_data::fs_walk(&resolved, self.policy.max_collection_items) {
                    Ok(entries) => {
                        let allocated = entries
                            .iter()
                            .map(|entry| {
                                std::mem::size_of::<stdlib_data::DirectoryEntry>()
                                    .saturating_add(entry.relative_path.len())
                                    .saturating_add(entry.kind.len())
                            })
                            .sum::<usize>();
                        charge_allocation(budget.as_ref(), allocated, instruction.span)?;
                        let values = entries
                            .into_iter()
                            .map(|entry| {
                                Value::Record(
                                    "DirectoryEntry".into(),
                                    Shared::new(vec![
                                        Value::Text(entry.relative_path.into()),
                                        Value::Text(entry.kind.into()),
                                    ]),
                                )
                            })
                            .collect();
                        push_stack!(Value::ResultOk(Box::new(Value::from_list_items(values))));
                    }
                    Err(error) if error.code == "E_FS_WALK_LIMIT" => {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            error.message,
                            instruction.span,
                        ));
                    }
                    Err(error) => push_stack!(file_error_result(RuntimeError::new(
                        error.code,
                        error.message,
                        instruction.span
                    ))),
                }
            }
            Op::ListGetOption => {
                let index = match pop!() {
                    Value::Int(index) => index.to_usize(),
                    _ => None,
                };
                let list = pop!();
                let Some(index) = index else {
                    push_stack!(Value::OptionNone);
                    return Ok(());
                };
                let value = list.list_item(index);
                push_stack!(value
                    .map(|value| Value::OptionSome(Box::new(value)))
                    .unwrap_or(Value::OptionNone));
            }
            Op::MapGetOption => {
                let key = match pop!() {
                    Value::Text(key) => key,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "map_get key must be Text",
                            instruction.span,
                        ));
                    }
                };
                let map = match pop!() {
                    Value::Map(map) => map,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "map_get requires Map",
                            instruction.span,
                        ));
                    }
                };
                push_stack!(map
                    .get(key.as_ref())
                    .cloned()
                    .map(|value| Value::OptionSome(Box::new(value)))
                    .unwrap_or(Value::OptionNone));
            }
            Op::TextParseInt | Op::TextParseFloat => {
                let text = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "numeric parser requires Text",
                            instruction.span,
                        ));
                    }
                };
                let parsed: Result<Value, stdlib_data::DataError> =
                    if instruction.op == Op::TextParseInt {
                        stdlib_data::text_parse_int(&text).map(Value::Int)
                    } else {
                        stdlib_data::text_parse_float(&text).and_then(|value| {
                            Float64::new(value).map(Value::Float64).ok_or_else(|| {
                                stdlib_data::DataError {
                                    code: "E_FLOAT_PARSE",
                                    message: "floating-point value must be finite".into(),
                                }
                            })
                        })
                    };
                match parsed {
                    Ok(value) => {
                        if let Value::Int(integer) = &value {
                            if integer.digit_count() > self.policy.max_integer_digits {
                                return Err(RuntimeError::new(
                                    "R_INTEGER_LIMIT",
                                    "parsed integer exceeds the configured digit limit",
                                    instruction.span,
                                ));
                            }
                        }
                        push_stack!(Value::ResultOk(Box::new(value)));
                    }
                    Err(error) => push_stack!(file_error_result(RuntimeError::new(
                        error.code,
                        error.message,
                        instruction.span
                    ))),
                }
            }
            Op::TextCaseFold | Op::TextCollapseWhitespace => {
                let text = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "Unicode operation requires Text",
                            instruction.span,
                        ));
                    }
                };
                if instruction.op == Op::TextCaseFold {
                    let output = stdlib_services::text_casefold(&text);
                    if output.len() > self.policy.max_text_bytes {
                        return Err(RuntimeError::new(
                            "R_TEXT_LIMIT",
                            "Unicode result exceeds the configured text limit",
                            instruction.span,
                        ));
                    }
                    charge_allocation(budget.as_ref(), output.len(), instruction.span)?;
                    push_stack!(Value::Text(output.into()));
                } else if let Some(output) =
                    stdlib_services::text_collapse_whitespace_if_changed(&text)
                {
                    if output.len() > self.policy.max_text_bytes {
                        return Err(RuntimeError::new(
                            "R_TEXT_LIMIT",
                            "Unicode result exceeds the configured text limit",
                            instruction.span,
                        ));
                    }
                    charge_allocation(budget.as_ref(), output.len(), instruction.span)?;
                    push_stack!(Value::Text(output.into()));
                } else {
                    push_stack!(Value::Text(text));
                }
            }
            Op::TextNormalize => {
                let form = match pop!() {
                    Value::Text(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "normalization form must be Text",
                            instruction.span,
                        ));
                    }
                };
                let text = match pop!() {
                    Value::Text(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "normalization input must be Text",
                            instruction.span,
                        ));
                    }
                };
                match stdlib_services::text_normalize(&text, &form) {
                    Ok(output) if output.len() <= self.policy.max_text_bytes => {
                        charge_allocation(budget.as_ref(), output.len(), instruction.span)?;
                        push_stack!(Value::ResultOk(Box::new(Value::Text(output.into()))));
                    }
                    Ok(_) => {
                        return Err(RuntimeError::new(
                            "R_TEXT_LIMIT",
                            "normalized text exceeds the configured text limit",
                            instruction.span,
                        ));
                    }
                    Err(error) => {
                        push_stack!(Value::ResultErr(Box::new(service_error_value(error))))
                    }
                }
            }
            Op::CryptoHmacSha256 => {
                let body = match pop!() {
                    Value::Bytes(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "HMAC body must be Bytes",
                            instruction.span,
                        ));
                    }
                };
                let key = match pop!() {
                    Value::Bytes(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "HMAC key must be Bytes",
                            instruction.span,
                        ));
                    }
                };
                push_stack!(Value::Bytes(Shared::new(
                    stdlib_services::crypto_hmac_sha256(&key, &body)
                )));
            }
            Op::CryptoVerifyHmacSha256 => {
                let expected = match pop!() {
                    Value::Bytes(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "HMAC digest must be Bytes",
                            instruction.span,
                        ));
                    }
                };
                let body = match pop!() {
                    Value::Bytes(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "HMAC body must be Bytes",
                            instruction.span,
                        ));
                    }
                };
                let key = match pop!() {
                    Value::Bytes(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "HMAC key must be Bytes",
                            instruction.span,
                        ));
                    }
                };
                push_stack!(Value::Bool(stdlib_services::crypto_verify_hmac_sha256(
                    &key, &body, &expected
                )));
            }
            Op::SqliteOpen => {
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "sqlite_open path must be Text",
                            instruction.span,
                        ));
                    }
                };
                let result = if !self.policy.allow_file_read || !self.policy.allow_file_write {
                    Err(stdlib_services::ServiceError::new(
                        "E_SQLITE_CAPABILITY",
                        "SQLite requires both file read and file write grants",
                    ))
                } else {
                    match host_file_path(&self.policy, &path, true, instruction.span) {
                        Ok(path) => self.databases.open(path).map(|handle| {
                            let (owner, id) = handle.parts();
                            Value::NativeResource(Box::new(NativeResourceValue::new(
                                "Db", owner, id,
                            )))
                        }),
                        Err(error) => Err(stdlib_services::ServiceError::new(
                            "E_SQLITE_PATH",
                            error.message,
                        )
                        .at_path(path.to_string())),
                    }
                };
                push_stack!(service_result_value(result));
            }
            Op::SqliteBegin => {
                let (owner, id) = runtime_service_resource(&pop!(), "Db", instruction.span)?;
                let result = self
                    .databases
                    .begin(stdlib_services::DbHandle::from_parts(owner, id))
                    .and_then(|handle| {
                        let cancellation = budget.cancellation.cancelled.clone();
                        self.databases.set_cancelled(handle, cancellation)?;
                        let (owner, id) = handle.parts();
                        Ok(Value::NativeResource(Box::new(NativeResourceValue::new(
                            "Transaction",
                            owner,
                            id,
                        ))))
                    });
                push_stack!(service_result_value(result));
            }
            Op::SqliteExecute | Op::SqliteQuery => {
                let values = runtime_sql_values(&pop!(), instruction.span)?;
                let sql = match pop!() {
                    Value::Text(sql) => sql,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "SQLite query must be Text",
                            instruction.span,
                        ));
                    }
                };
                let (owner, id) =
                    runtime_service_resource(&pop!(), "Transaction", instruction.span)?;
                let transaction = stdlib_services::TransactionHandle::from_parts(owner, id);
                if instruction.op == Op::SqliteExecute {
                    let result = self
                        .databases
                        .execute(transaction, &sql, &values)
                        .map(|affected| Value::Int(BigInt::from_i64(affected)));
                    push_stack!(service_result_value(result));
                } else {
                    let result =
                        self.databases
                            .query(transaction, &sql, &values)
                            .and_then(|rows| {
                                if rows.len() > self.policy.max_collection_items {
                                    return Err(stdlib_services::ServiceError::new(
                                        "E_SQLITE_RESULT_LIMIT",
                                        "SQLite query exceeded the configured collection limit",
                                    ));
                                }
                                let values = rows
                                    .into_iter()
                                    .map(|row| {
                                        Value::Record(
                                            "SqlRow".into(),
                                            Shared::new(vec![
                                                Value::from_list_items(
                                                    row.columns
                                                        .into_iter()
                                                        .map(|text| Value::Text(text.into()))
                                                        .collect(),
                                                ),
                                                Value::from_list_items(
                                                    row.values
                                                        .into_iter()
                                                        .map(service_sql_value)
                                                        .collect(),
                                                ),
                                            ]),
                                        )
                                    })
                                    .collect::<Vec<_>>();
                                let bytes = values
                                    .iter()
                                    .map(cloned_value_allocation)
                                    .fold(0usize, usize::saturating_add);
                                charge_allocation(budget.as_ref(), bytes, instruction.span)
                                    .map_err(|error| {
                                        stdlib_services::ServiceError::new(
                                            error.code.as_str(),
                                            error.message,
                                        )
                                    })?;
                                Ok(Value::from_list_items(values))
                            });
                    push_stack!(service_result_value(result));
                }
            }
            Op::SqliteCommit | Op::SqliteRollback => {
                let (owner, id) =
                    runtime_service_resource(&pop!(), "Transaction", instruction.span)?;
                let transaction = stdlib_services::TransactionHandle::from_parts(owner, id);
                let result = if instruction.op == Op::SqliteCommit {
                    self.databases.commit(transaction)
                } else {
                    self.databases.rollback(transaction)
                }
                .map(|()| Value::Unit);
                push_stack!(service_result_value(result));
            }
            Op::SqliteClose => {
                let (owner, id) = runtime_service_resource(&pop!(), "Db", instruction.span)?;
                let result = self
                    .databases
                    .close(stdlib_services::DbHandle::from_parts(owner, id))
                    .map(|()| Value::Unit);
                push_stack!(service_result_value(result));
            }
            Op::TomlParse => {
                let source = match pop!() {
                    Value::Text(source) => source,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "toml_parse input must be Text",
                            instruction.span,
                        ));
                    }
                };
                let result = self.toml_documents.parse(&source).map(|handle| {
                    let (owner, id) = handle.parts();
                    Value::NativeResource(Box::new(NativeResourceValue::new(
                        "TomlDocument",
                        owner,
                        id,
                    )))
                });
                push_stack!(service_result_value(result));
            }
            Op::TomlHas | Op::TomlGetText | Op::TomlGetInt | Op::TomlGetBool => {
                let path = runtime_text_path(&pop!(), instruction.span)?;
                let (owner, id) =
                    runtime_service_resource(&pop!(), "TomlDocument", instruction.span)?;
                let handle = stdlib_services::TomlHandle::from_parts(owner, id);
                let result = match instruction.op {
                    Op::TomlHas => self.toml_documents.has(handle, &path).map(Value::Bool),
                    Op::TomlGetText => self
                        .toml_documents
                        .get_text(handle, &path)
                        .map(|text| Value::Text(text.into())),
                    Op::TomlGetInt => self
                        .toml_documents
                        .get_integer(handle, &path)
                        .map(|value| Value::Int(BigInt::from_i64(value))),
                    Op::TomlGetBool => self.toml_documents.get_bool(handle, &path).map(Value::Bool),
                    _ => unreachable!(),
                };
                if instruction.op == Op::TomlHas {
                    let value = result.map_err(|error| {
                        RuntimeError::new(error.code, error.message, instruction.span)
                    })?;
                    push_stack!(value);
                } else {
                    push_stack!(service_result_value(result));
                }
            }
            Op::TomlPutText | Op::TomlPutInt | Op::TomlPutBool => {
                let replacement = pop!();
                let path = runtime_text_path(&pop!(), instruction.span)?;
                let (owner, id) =
                    runtime_service_resource(&pop!(), "TomlDocument", instruction.span)?;
                let handle = stdlib_services::TomlHandle::from_parts(owner, id);
                let result = match (&instruction.op, replacement) {
                    (Op::TomlPutText, Value::Text(value)) => {
                        self.toml_documents.put_text(handle, &path, &value)
                    }
                    (Op::TomlPutInt, Value::Int(value)) => value
                        .to_i64()
                        .ok_or_else(|| {
                            stdlib_services::ServiceError::new(
                                "E_TOML_INTEGER_RANGE",
                                "TOML integers must fit signed 64-bit range",
                            )
                        })
                        .and_then(|value| self.toml_documents.put_integer(handle, &path, value)),
                    (Op::TomlPutBool, Value::Bool(value)) => {
                        self.toml_documents.put_bool(handle, &path, value)
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "TOML value type does not match the edit operation",
                            instruction.span,
                        ));
                    }
                }
                .map(|()| Value::Unit);
                push_stack!(service_result_value(result));
            }
            Op::TomlRender => {
                let (owner, id) =
                    runtime_service_resource(&pop!(), "TomlDocument", instruction.span)?;
                let text = self
                    .toml_documents
                    .render(stdlib_services::TomlHandle::from_parts(owner, id))
                    .map_err(|error| {
                        RuntimeError::new("R_TOML_RENDER", error.to_string(), instruction.span)
                    })?;
                if text.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "rendered TOML exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    size_of::<String>().saturating_add(text.len()),
                    instruction.span,
                )?;
                push_stack!(Value::Text(text.into()));
            }
            Op::CalendarParse => {
                let source = match pop!() {
                    Value::Text(source) => source,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "calendar_parse input must be Text",
                            instruction.span,
                        ));
                    }
                };
                match stdlib_services::calendar_parse(&source) {
                    Ok(events) if events.len() <= self.policy.max_collection_items => {
                        let values = events
                            .into_iter()
                            .map(service_calendar_event)
                            .collect::<Vec<_>>();
                        let bytes = values
                            .iter()
                            .map(cloned_value_allocation)
                            .fold(0usize, usize::saturating_add);
                        charge_allocation(budget.as_ref(), bytes, instruction.span)?;
                        push_stack!(Value::ResultOk(Box::new(Value::from_list_items(values))));
                    }
                    Ok(_) => push_stack!(Value::ResultErr(Box::new(service_error_value(
                        stdlib_services::ServiceError::new(
                            "E_ICAL_LIMIT",
                            "calendar event count exceeds the configured collection limit"
                        )
                    )))),
                    Err(error) => {
                        push_stack!(Value::ResultErr(Box::new(service_error_value(error))))
                    }
                }
            }
            Op::ReadText => {
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "read_text path must be Text",
                            instruction.span,
                        ));
                    }
                };
                let resolved = match host_file_path(&self.policy, &path, false, instruction.span) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                let metadata = match fs::metadata(&resolved) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        push_stack!(file_error_result(RuntimeError::new(
                            file_io_error_code(&error, "R_FILE_READ"),
                            format!("cannot inspect `{path}`: {error}"),
                            instruction.span,
                        )));
                        return Ok(());
                    }
                };
                if metadata.len() as usize > self.policy.max_file_bytes {
                    return Err(RuntimeError::new(
                        "R_FILE_LIMIT",
                        format!("file `{path}` exceeds the configured byte limit"),
                        instruction.span,
                    ));
                }
                let text = match fs::read_to_string(&resolved) {
                    Ok(text) => text,
                    Err(error) => {
                        push_stack!(file_error_result(RuntimeError::new(
                            file_io_error_code(&error, "R_FILE_READ"),
                            format!("cannot read `{path}` as UTF-8 text: {error}"),
                            instruction.span,
                        )));
                        return Ok(());
                    }
                };
                if text.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "file content exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    size_of::<String>().saturating_add(text.len()),
                    instruction.span,
                )?;
                let effect = self.begin_effect(EffectKind::FileRead, path);
                self.finish_effect(effect, EffectOutcome::Returned);
                push_stack!(Value::ResultOk(Box::new(Value::Text(text.into()))));
            }
            Op::WriteText => {
                let content = match pop!() {
                    Value::Text(content) => content,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "write_text content must be Text",
                            instruction.span,
                        ));
                    }
                };
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "write_text path must be Text",
                            instruction.span,
                        ));
                    }
                };
                if content.len() > self.policy.max_file_bytes {
                    return Err(RuntimeError::new(
                        "R_FILE_LIMIT",
                        "file content exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                let resolved = match host_file_path(&self.policy, &path, true, instruction.span) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                let effect = self.begin_effect(EffectKind::FileWrite, path.as_ref());
                if let Err(error) = fs::write(&resolved, content.as_bytes()) {
                    self.finish_effect(effect, EffectOutcome::ReturnedError);
                    push_stack!(file_error_result(RuntimeError::new(
                        "R_FILE_WRITE",
                        format!("cannot write `{path}`: {error}"),
                        instruction.span,
                    )));
                    return Ok(());
                }
                self.finish_effect(effect, EffectOutcome::Returned);
                push_stack!(Value::ResultOk(Box::new(Value::Unit)));
            }
            Op::WriteTextAtomic => {
                let content = match pop!() {
                    Value::Text(content) => content,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "write_text_atomic content must be Text",
                            instruction.span,
                        ));
                    }
                };
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "write_text_atomic path must be Text",
                            instruction.span,
                        ));
                    }
                };
                if content.len() > self.policy.max_file_bytes {
                    return Err(RuntimeError::new(
                        "R_FILE_LIMIT",
                        "file content exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                if content.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "file content exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                let resolved = match host_file_path(&self.policy, &path, true, instruction.span) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                let effect = self.begin_effect(EffectKind::FileWrite, path.as_ref());
                if let Err(error) = atomic_replace_text(&resolved, &content) {
                    self.finish_effect(effect, EffectOutcome::ReturnedError);
                    push_stack!(file_error_result(RuntimeError::new(
                        file_io_error_code(&error, "R_FILE_WRITE"),
                        format!("cannot atomically replace `{path}`: {error}"),
                        instruction.span,
                    )));
                    return Ok(());
                }
                self.finish_effect(effect, EffectOutcome::Returned);
                push_stack!(Value::ResultOk(Box::new(Value::Unit)));
            }
            Op::ReadBytes => {
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "read_bytes path must be Text",
                            instruction.span,
                        ));
                    }
                };
                let resolved = match host_file_path(&self.policy, &path, false, instruction.span) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                let metadata = match fs::metadata(&resolved) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        push_stack!(file_error_result(RuntimeError::new(
                            file_io_error_code(&error, "R_FILE_READ"),
                            format!("cannot inspect `{path}`: {error}"),
                            instruction.span
                        )));
                        return Ok(());
                    }
                };
                if metadata.len() > self.policy.max_file_bytes as u64 {
                    return Err(RuntimeError::new(
                        "R_FILE_LIMIT",
                        format!("file `{path}` exceeds the configured byte limit"),
                        instruction.span,
                    ));
                }
                let bytes = match fs::read(&resolved) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        push_stack!(file_error_result(RuntimeError::new(
                            file_io_error_code(&error, "R_FILE_READ"),
                            format!("cannot read `{path}`: {error}"),
                            instruction.span
                        )));
                        return Ok(());
                    }
                };
                if bytes.len() > self.policy.max_file_bytes {
                    return Err(RuntimeError::new(
                        "R_FILE_LIMIT",
                        "file content exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    bytes.capacity().saturating_add(size_of::<Vec<u8>>()),
                    instruction.span,
                )?;
                let effect = self.begin_effect(EffectKind::FileRead, path.as_ref());
                self.finish_effect(effect, EffectOutcome::Returned);
                push_stack!(Value::ResultOk(Box::new(Value::Bytes(Shared::new(bytes)))));
            }
            Op::WriteBytes => {
                let content = match pop!() {
                    Value::Bytes(content) => content,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "write_bytes content must be Bytes",
                            instruction.span,
                        ));
                    }
                };
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "write_bytes path must be Text",
                            instruction.span,
                        ));
                    }
                };
                if content.len() > self.policy.max_file_bytes {
                    return Err(RuntimeError::new(
                        "R_FILE_LIMIT",
                        "file content exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                let resolved = match host_file_path(&self.policy, &path, true, instruction.span) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                let effect = self.begin_effect(EffectKind::FileWrite, path.as_ref());
                if let Err(error) = fs::write(&resolved, content.as_slice()) {
                    self.finish_effect(effect, EffectOutcome::ReturnedError);
                    push_stack!(file_error_result(RuntimeError::new(
                        "R_FILE_WRITE",
                        format!("cannot write `{path}`: {error}"),
                        instruction.span
                    )));
                    return Ok(());
                }
                self.finish_effect(effect, EffectOutcome::Returned);
                push_stack!(Value::ResultOk(Box::new(Value::Unit)));
            }
            Op::Args => {
                let allocation = self.policy.arguments.iter().fold(
                    size_of::<Vec<Value>>().saturating_add(
                        self.policy
                            .arguments
                            .len()
                            .saturating_mul(size_of::<Value>()),
                    ),
                    |total, value| total.saturating_add(size_of::<String>() + value.len()),
                );
                charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                let values = self
                    .policy
                    .arguments
                    .iter()
                    .cloned()
                    .map(|value| Value::Text(value.into()))
                    .collect::<Vec<_>>();
                if values.len() > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "process arguments exceed the configured item limit",
                        instruction.span,
                    ));
                }
                if let Some(error) = values
                    .iter()
                    .find_map(|value| value_limit_error(value, &self.policy, instruction.span))
                {
                    return Err(error);
                }
                push_stack!(Value::List(Shared::new(values)));
            }
            Op::ArgsFrom => {
                let start = match pop!() {
                    Value::Int(index) => index
                        .to_usize()
                        .filter(|index| *index <= self.policy.arguments.len())
                        .ok_or_else(|| {
                            RuntimeError::new(
                                "R_ARGUMENT_INDEX",
                                "args_from index must be between zero and the argument count",
                                instruction.span,
                            )
                        })?,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "args_from requires an Int index",
                            instruction.span,
                        ));
                    }
                };
                let values = self
                    .policy
                    .arguments
                    .iter()
                    .skip(start)
                    .cloned()
                    .map(|value| Value::Text(value.into()))
                    .collect::<Vec<_>>();
                let allocation = values.iter().fold(
                    size_of::<Vec<Value>>()
                        .saturating_add(values.len().saturating_mul(size_of::<Value>())),
                    |total, value| match value {
                        Value::Text(text) => total.saturating_add(size_of::<String>() + text.len()),
                        _ => total,
                    },
                );
                charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                if values.len() > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "process arguments exceed the configured item limit",
                        instruction.span,
                    ));
                }
                if let Some(error) = values
                    .iter()
                    .find_map(|value| value_limit_error(value, &self.policy, instruction.span))
                {
                    return Err(error);
                }
                push_stack!(Value::List(Shared::new(values)));
            }
            Op::Environment => {
                let name = match pop!() {
                    Value::Text(name) => name,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "env name must be Text",
                            instruction.span,
                        ));
                    }
                };
                if !self
                    .policy
                    .allowed_environment_variables
                    .contains(name.as_ref())
                {
                    return Err(RuntimeError::new(
                        "R_ENV_DENIED",
                        format!("environment variable `{name}` is not granted"),
                        instruction.span,
                    ));
                }
                let effect = self.begin_effect(EffectKind::EnvironmentRead, name.as_ref());
                match std::env::var(name.as_ref()) {
                    Ok(value) => {
                        self.finish_effect(effect, EffectOutcome::Returned);
                        push_stack!(Value::OptionSome(Box::new(Value::Text(value.into()))))
                    }
                    Err(std::env::VarError::NotPresent) => {
                        self.finish_effect(effect, EffectOutcome::Returned);
                        push_stack!(Value::OptionNone)
                    }
                    Err(std::env::VarError::NotUnicode(_)) => {
                        self.finish_effect(effect, EffectOutcome::ReturnedError);
                        return Err(RuntimeError::new(
                            "R_ENV_ENCODING",
                            "environment value is not valid Unicode",
                            instruction.span,
                        ));
                    }
                }
            }
            Op::JsonErrorMessage | Op::JsonErrorLine | Op::JsonErrorColumn | Op::JsonErrorCode => {
                let error = match pop!() {
                    Value::JsonError(error) => error,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "JSON error accessor requires JsonError",
                            instruction.span,
                        ));
                    }
                };
                push_stack!(match instruction.op {
                    Op::JsonErrorMessage => Value::Text(error.message.into()),
                    Op::JsonErrorLine => Value::Int(BigInt::from_usize(error.line)),
                    Op::JsonErrorColumn => Value::Int(BigInt::from_usize(error.column)),
                    Op::JsonErrorCode => Value::Text(error.code.into()),
                    _ => unreachable!(),
                });
            }
            Op::FileErrorCode | Op::FileErrorMessage | Op::FileErrorLine | Op::FileErrorColumn => {
                let error = match pop!() {
                    Value::FileError(error) => error,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "file error accessor requires FileError",
                            instruction.span,
                        ));
                    }
                };
                push_stack!(match instruction.op {
                    Op::FileErrorCode => Value::Text(error.code.into()),
                    Op::FileErrorMessage => Value::Text(error.message.into()),
                    Op::FileErrorLine => Value::Int(BigInt::from_usize(
                        error.trace.first().map(|span| span.line).unwrap_or(0),
                    )),
                    Op::FileErrorColumn => Value::Int(BigInt::from_usize(
                        error.trace.first().map(|span| span.column).unwrap_or(0),
                    )),
                    _ => unreachable!(),
                });
            }
            Op::HttpErrorCode | Op::HttpErrorMessage => {
                let error = match pop!() {
                    Value::HttpError(error) => error,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "HTTP error accessor requires HttpError",
                            instruction.span,
                        ));
                    }
                };
                push_stack!(Value::Text(
                    if matches!(instruction.op, Op::HttpErrorCode) {
                        error.code.into()
                    } else {
                        error.message.into()
                    }
                ));
            }
            Op::TaskIndex => match pop!() {
                Value::TaskError(error) => push_stack!(Value::Int(error.index)),
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "task_index requires TaskError",
                        instruction.span,
                    ));
                }
            },
            Op::TaskCode => match pop!() {
                Value::TaskError(error) => push_stack!(Value::Text(error.code.into())),
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "task_code requires TaskError",
                        instruction.span,
                    ));
                }
            },
            Op::TaskMessage => match pop!() {
                Value::TaskError(error) => push_stack!(Value::Text(error.message.into())),
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "task_message requires TaskError",
                        instruction.span,
                    ));
                }
            },
            Op::PythonCall(target, argc) => {
                let mut call_args = Vec::with_capacity(argc);
                for _ in 0..argc {
                    call_args.push(pop!());
                }
                call_args.reverse();
                let result =
                    self.call_python(target, &call_args, &[], budget.as_ref(), instruction.span)?;
                push_stack!(result);
            }
            Op::PythonCallKw(target, positional_count, ref keyword_names) => {
                let mut supplied = Vec::with_capacity(positional_count + keyword_names.len());
                for _ in 0..positional_count + keyword_names.len() {
                    supplied.push(pop!());
                }
                supplied.reverse();
                let keyword_values = supplied.split_off(positional_count);
                let call_kwargs = keyword_names
                    .iter()
                    .cloned()
                    .zip(keyword_values)
                    .collect::<Vec<_>>();
                let result = self.call_python(
                    target,
                    &supplied,
                    &call_kwargs,
                    budget.as_ref(),
                    instruction.span,
                )?;
                push_stack!(result);
            }
            Op::JsonNull => {
                push_stack!(finish_json_value(
                    serde_json::Value::Null,
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonBool => {
                let boolean = match pop!() {
                    Value::Bool(boolean) => boolean,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_bool requires Bool",
                            instruction.span,
                        ));
                    }
                };
                push_stack!(finish_json_value(
                    serde_json::Value::Bool(boolean),
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonInt => {
                let integer = match pop!() {
                    Value::Int(integer) => integer,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_int requires Int",
                            instruction.span,
                        ));
                    }
                };
                if integer.digit_count() > self.policy.max_integer_digits {
                    return Err(RuntimeError::new(
                        "R_INTEGER_LIMIT",
                        "JSON integer exceeds the configured digit limit",
                        instruction.span,
                    ));
                }
                let parsed = serde_json::from_str::<serde_json::Value>(&integer.to_string())
                    .map_err(|error| {
                        RuntimeError::new("R_JSON_ENCODE", error.to_string(), instruction.span)
                    })?;
                push_stack!(finish_json_value(
                    parsed,
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonFloat => {
                let float = match pop!() {
                    Value::Float64(float) => float.get(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_float requires Float64",
                            instruction.span,
                        ));
                    }
                };
                let number = serde_json::Number::from_f64(float).ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "JSON number must be finite",
                        instruction.span,
                    )
                })?;
                push_stack!(finish_json_value(
                    serde_json::Value::Number(number),
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonText => {
                let text = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_text requires Text",
                            instruction.span,
                        ));
                    }
                };
                if text.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "JSON text exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                push_stack!(finish_json_value(
                    serde_json::Value::String(text.to_string()),
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonArray => {
                push_stack!(finish_json_value(
                    serde_json::Value::Array(Vec::new()),
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonObject => {
                push_stack!(finish_json_value(
                    serde_json::Value::Object(serde_json::Map::new()),
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonPush => {
                let child = match pop!() {
                    Value::Json(value) => value.as_ref().clone(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_push value must be Json",
                            instruction.span,
                        ));
                    }
                };
                let mut array = match pop!() {
                    Value::Json(value) => value.as_ref().clone(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_push requires a JSON array",
                            instruction.span,
                        ));
                    }
                };
                let Some(items) = array.as_array_mut() else {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_push requires an array",
                        instruction.span,
                    ));
                };
                items.push(Shared::new(child));
                push_stack!(finish_shared_json(
                    array,
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonSet => {
                let child = match pop!() {
                    Value::Json(value) => value.as_ref().clone(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_set value must be Json",
                            instruction.span,
                        ));
                    }
                };
                let key = match pop!() {
                    Value::Text(key) => key,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_set key must be Text",
                            instruction.span,
                        ));
                    }
                };
                if key.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "JSON key exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                let mut object = match pop!() {
                    Value::Json(value) => value.as_ref().clone(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_set requires a JSON object",
                            instruction.span,
                        ));
                    }
                };
                let Some(fields) = object.as_object_mut() else {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_set requires an object",
                        instruction.span,
                    ));
                };
                fields.insert(key.to_string(), Shared::new(child));
                push_stack!(finish_shared_json(
                    object,
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonParse => {
                let source = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_parse requires Text",
                            instruction.span,
                        ));
                    }
                };
                if source.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "JSON input exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                let parse_started = budget.runtime_profile.as_ref().map(|_| Instant::now());
                let parsed = parse_unique_shared_json(&source);
                if let (Some(profile), Some(started)) = (&budget.runtime_profile, parse_started) {
                    profile.record_json_parse(
                        started.elapsed().as_nanos().min(u64::MAX as u128) as u64
                    );
                }
                let value = match parsed {
                    Ok(value) => value,
                    Err(JsonDecodeError::Parse(error)) => {
                        push_stack!(Value::ResultErr(Box::new(Value::JsonError(Box::new(
                            JsonErrorValue {
                                code: "R_JSON_PARSE".into(),
                                message: error.to_string(),
                                line: error.line(),
                                column: error.column(),
                            },
                        )))));
                        return Ok(());
                    }
                    Err(JsonDecodeError::DuplicateKey { key, line, column }) => {
                        push_stack!(Value::ResultErr(Box::new(Value::JsonError(Box::new(
                            JsonErrorValue {
                                code: "R_JSON_DUPLICATE_KEY".into(),
                                message: format!("JSON object contains duplicate key `{key}`"),
                                line,
                                column,
                            },
                        )))));
                        return Ok(());
                    }
                };
                let value = finish_shared_json(
                    value,
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?;
                push_stack!(Value::ResultOk(Box::new(value)));
            }
            Op::JsonStringify => {
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_stringify requires Json",
                            instruction.span,
                        ));
                    }
                };
                let text = serde_json::to_string(value.as_ref()).map_err(|error| {
                    RuntimeError::new("R_JSON_ENCODE", error.to_string(), instruction.span)
                })?;
                if text.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "JSON output exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    size_of::<String>().saturating_add(text.len()),
                    instruction.span,
                )?;
                push_stack!(Value::Text(text.into()));
            }
            Op::JsonStringifyPairs => {
                let value_field = match pop!() {
                    Value::Text(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_stringify_pairs value field must be Text",
                            instruction.span,
                        ));
                    }
                };
                let key_field = match pop!() {
                    Value::Text(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_stringify_pairs key field must be Text",
                            instruction.span,
                        ));
                    }
                };
                let groups = match pop!() {
                    Value::Map(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_stringify_pairs requires Map[Text, List[Text]]",
                            instruction.span,
                        ));
                    }
                };
                if key_field == value_field {
                    return Err(RuntimeError::new(
                        "R_JSON_ENCODE",
                        "json_stringify_pairs field names must be distinct",
                        instruction.span,
                    ));
                }
                if groups.len() > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "json_stringify_pairs exceeds the configured group limit",
                        instruction.span,
                    ));
                }

                let encoded_key_field_len = encoded_json_string_len(&key_field);
                let encoded_value_field_len = encoded_json_string_len(&value_field);
                let mut row_count = 0usize;
                let mut output_length = 2usize;
                for (key, values) in groups.iter() {
                    let Value::List(values) = values else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_stringify_pairs requires Map[Text, List[Text]]",
                            instruction.span,
                        ));
                    };
                    if values.len() > self.policy.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "json_stringify_pairs exceeds the configured group size limit",
                            instruction.span,
                        ));
                    }
                    let encoded_key_len = encoded_json_string_len(key);
                    for value in values.iter() {
                        let Value::Text(value) = value else {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "json_stringify_pairs requires Map[Text, List[Text]]",
                                instruction.span,
                            ));
                        };
                        if row_count >= self.policy.max_collection_items {
                            return Err(RuntimeError::new(
                                "R_COLLECTION_LIMIT",
                                "json_stringify_pairs exceeds the configured row limit",
                                instruction.span,
                            ));
                        }
                        if row_count > 0 {
                            output_length = output_length.saturating_add(1);
                        }
                        output_length = output_length
                            .saturating_add(5)
                            .saturating_add(encoded_key_field_len)
                            .saturating_add(encoded_key_len)
                            .saturating_add(encoded_value_field_len)
                            .saturating_add(encoded_json_string_len(value));
                        row_count += 1;
                        if output_length > self.policy.max_text_bytes {
                            return Err(RuntimeError::new(
                                "R_TEXT_LIMIT",
                                "JSON pair output exceeds the configured text limit",
                                instruction.span,
                            ));
                        }
                    }
                }
                if output_length > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "JSON pair output exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    size_of::<String>().saturating_add(output_length),
                    instruction.span,
                )?;

                let mut output = Vec::with_capacity(output_length);
                let rows = JsonTextPairRows {
                    groups: groups.as_ref(),
                    key_field: &key_field,
                    value_field: &value_field,
                    row_count,
                };
                let mut serializer = serde_json::Serializer::new(&mut output);
                serde::Serialize::serialize(&rows, &mut serializer).map_err(|error| {
                    RuntimeError::new("R_JSON_ENCODE", error.to_string(), instruction.span)
                })?;
                if output.len() != output_length {
                    return Err(RuntimeError::new(
                        "R_JSON_ENCODE",
                        "JSON pair output length did not match its checked size",
                        instruction.span,
                    ));
                }
                let text = String::from_utf8(output).map_err(|error| {
                    RuntimeError::new("R_JSON_ENCODE", error.to_string(), instruction.span)
                })?;
                push_stack!(Value::Text(text.into()));
            }
            Op::JsonTryGet
            | Op::JsonTryGetText
            | Op::JsonTryGetInt
            | Op::JsonTryIndex
            | Op::JsonTryText
            | Op::JsonTryInt
            | Op::JsonTryBool
            | Op::JsonTryFloat => {
                let outcome: Result<Value, JsonErrorValue> = match instruction.op {
                    Op::JsonTryGet | Op::JsonTryGetText | Op::JsonTryGetInt => {
                        let key = match pop!() {
                            Value::Text(key) => key,
                            _ => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "json_try_get key must be Text",
                                    instruction.span,
                                ));
                            }
                        };
                        if key.len() > self.policy.max_text_bytes {
                            return Err(RuntimeError::new(
                                "R_TEXT_LIMIT",
                                "JSON key exceeds the configured byte limit",
                                instruction.span,
                            ));
                        }
                        let value = match pop!() {
                            Value::Json(value) => value,
                            _ => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "json_try_get requires Json",
                                    instruction.span,
                                ));
                            }
                        };
                        match instruction.op {
                            Op::JsonTryGet => {
                                stdlib_data::json_try_get(&value, &key).map(Value::Json)
                            }
                            Op::JsonTryGetText => {
                                stdlib_data::json_try_get(&value, &key).and_then(|child| {
                                    stdlib_data::json_try_text(child.as_ref())
                                        .map(|text| Value::Text(text.into()))
                                })
                            }
                            _ => stdlib_data::json_try_get(&value, &key).and_then(|child| {
                                stdlib_data::json_try_int(child.as_ref()).map(Value::Int)
                            }),
                        }
                    }
                    Op::JsonTryIndex => {
                        let index = match pop!() {
                            Value::Int(index) => index.to_i64().unwrap_or(-1),
                            _ => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "json_try_index index must be Int",
                                    instruction.span,
                                ));
                            }
                        };
                        let value = match pop!() {
                            Value::Json(value) => value,
                            _ => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "json_try_index requires Json",
                                    instruction.span,
                                ));
                            }
                        };
                        stdlib_data::json_try_index(&value, index).map(Value::Json)
                    }
                    Op::JsonTryText | Op::JsonTryInt | Op::JsonTryBool | Op::JsonTryFloat => {
                        let value = match pop!() {
                            Value::Json(value) => value,
                            _ => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "JSON conversion requires Json",
                                    instruction.span,
                                ));
                            }
                        };
                        match instruction.op {
                            Op::JsonTryText => stdlib_data::json_try_text(value.as_ref())
                                .map(|text| Value::Text(text.into())),
                            Op::JsonTryInt => {
                                stdlib_data::json_try_int(value.as_ref()).map(Value::Int)
                            }
                            Op::JsonTryBool => {
                                stdlib_data::json_try_bool(value.as_ref()).map(Value::Bool)
                            }
                            Op::JsonTryFloat => stdlib_data::json_try_float(value.as_ref())
                                .and_then(|number| {
                                    Float64::new(number).map(Value::Float64).ok_or_else(|| {
                                        JsonErrorValue {
                                            code: "E_JSON_TYPE".into(),
                                            message: "JSON number is outside finite Float64 range"
                                                .into(),
                                            line: 0,
                                            column: 0,
                                        }
                                    })
                                }),
                            _ => unreachable!(),
                        }
                    }
                    _ => unreachable!(),
                };
                match outcome {
                    Ok(value) => {
                        if let Value::Int(integer) = &value {
                            if integer.digit_count() > self.policy.max_integer_digits {
                                return Err(RuntimeError::new(
                                    "R_INTEGER_LIMIT",
                                    "JSON integer exceeds the configured digit limit",
                                    instruction.span,
                                ));
                            }
                        }
                        if let Value::Text(text) = &value {
                            charge_allocation(
                                budget.as_ref(),
                                size_of::<String>().saturating_add(text.len()),
                                instruction.span,
                            )?;
                        }
                        push_stack!(Value::ResultOk(Box::new(value)));
                    }
                    Err(error) => {
                        push_stack!(Value::ResultErr(Box::new(Value::JsonError(Box::new(
                            error
                        )))))
                    }
                }
            }
            Op::JsonGet => {
                let key = match pop!() {
                    Value::Text(key) => key,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_get key must be Text",
                            instruction.span,
                        ));
                    }
                };
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_get requires Json",
                            instruction.span,
                        ));
                    }
                };
                let object = value.as_object().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_get requires an object",
                        instruction.span,
                    )
                })?;
                let value = object.get(key.as_ref()).cloned().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_KEY",
                        format!("JSON object has no key `{key}`"),
                        instruction.span,
                    )
                })?;
                push_stack!(Value::Json(value));
            }
            Op::JsonIndex => {
                let index = match pop!() {
                    Value::Int(index) => index.to_usize(),
                    _ => None,
                }
                .ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_INDEX",
                        "JSON array index must be a non-negative machine-sized Int",
                        instruction.span,
                    )
                })?;
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_index requires Json",
                            instruction.span,
                        ));
                    }
                };
                let array = value.as_array().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_index requires an array",
                        instruction.span,
                    )
                })?;
                let value = array.get(index).cloned().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_INDEX",
                        format!(
                            "JSON array index {index} is out of range for length {}",
                            array.len()
                        ),
                        instruction.span,
                    )
                })?;
                push_stack!(Value::Json(value));
            }
            Op::JsonHas => {
                let key = match pop!() {
                    Value::Text(key) => key,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_has key must be Text",
                            instruction.span,
                        ));
                    }
                };
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_has requires Json",
                            instruction.span,
                        ));
                    }
                };
                let object = value.as_object().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_has requires an object",
                        instruction.span,
                    )
                })?;
                push_stack!(Value::Bool(object.contains_key(key.as_ref())));
            }
            Op::JsonLength => {
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_len requires Json",
                            instruction.span,
                        ));
                    }
                };
                let length = value
                    .as_array()
                    .map(|items| items.len())
                    .or_else(|| value.as_object().map(|items| items.len()))
                    .ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "json_len requires an array or object",
                            instruction.span,
                        )
                    })?;
                push_stack!(Value::Int(BigInt::from_usize(length)));
            }
            Op::JsonIntColumn => {
                let field = match pop!() {
                    Value::Text(field) => field,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_int_column requires Text field",
                            instruction.span,
                        ));
                    }
                };
                let rows = match pop!() {
                    Value::Json(rows) => rows,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_int_column requires Json rows",
                            instruction.span,
                        ));
                    }
                };
                let rows = rows.as_array().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_int_column requires an array of objects",
                        instruction.span,
                    )
                })?;
                if rows.len() > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "JSON column exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                let mut values = Vec::with_capacity(rows.len());
                let mut allocation = size_of::<Vec<Value>>()
                    .saturating_add(rows.len().saturating_mul(size_of::<Value>()));
                for row in rows {
                    let object = row.as_object().ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "json_int_column rows must be objects",
                            instruction.span,
                        )
                    })?;
                    let value = object.get(field.as_ref()).ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_FIELD",
                            format!("JSON row has no field {field:?}"),
                            instruction.span,
                        )
                    })?;
                    let integer = json_exact_integer(value).ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            format!("JSON field {field:?} must contain an exact integer"),
                            instruction.span,
                        )
                    })?;
                    if integer.digit_count() > self.policy.max_integer_digits {
                        return Err(RuntimeError::new(
                            "R_INTEGER_LIMIT",
                            "JSON integer exceeds the configured digit limit",
                            instruction.span,
                        ));
                    }
                    allocation = allocation
                        .saturating_add(size_of::<Value>())
                        .saturating_add(integer.digit_count());
                    values.push(Value::Int(integer));
                }
                charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                push_stack!(Value::List(Shared::new(values)));
            }
            Op::JsonTextCategories => {
                let field = match pop!() {
                    Value::Text(field) => field,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_text_categories requires Text field",
                            instruction.span,
                        ));
                    }
                };
                let rows = match pop!() {
                    Value::Json(rows) => rows,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_text_categories requires Json rows",
                            instruction.span,
                        ));
                    }
                };
                let rows = rows.as_array().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_text_categories requires an array of objects",
                        instruction.span,
                    )
                })?;
                if rows.len() > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "JSON category column exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                let mut codes = Vec::with_capacity(rows.len());
                let mut labels = Vec::<String>::new();
                let mut category_codes = HashMap::<String, usize>::new();
                for row in rows {
                    let object = row.as_object().ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "json_text_categories rows must be objects",
                            instruction.span,
                        )
                    })?;
                    let value = object.get(field.as_ref()).ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_FIELD",
                            format!("JSON row has no field {field:?}"),
                            instruction.span,
                        )
                    })?;
                    let label = value.as_str().ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            format!("JSON field {field:?} must contain Text"),
                            instruction.span,
                        )
                    })?;
                    let code = if let Some(code) = category_codes.get(label) {
                        *code
                    } else {
                        let code = labels.len();
                        if code >= self.policy.max_collection_items {
                            return Err(RuntimeError::new(
                                "R_COLLECTION_LIMIT",
                                "JSON categories exceed the configured item limit",
                                instruction.span,
                            ));
                        }
                        charge_allocation(
                            budget.as_ref(),
                            size_of::<(String, usize)>()
                                .saturating_add(label.len().saturating_mul(2)),
                            instruction.span,
                        )?;
                        let label = label.to_owned();
                        category_codes.insert(label.clone(), code);
                        labels.push(label);
                        code
                    };
                    codes.push(serde_json::Value::Number(serde_json::Number::from(
                        code as u64,
                    )));
                }
                let labels = labels
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect::<Vec<_>>();
                let value = serde_json::json!({ "codes": codes, "labels": labels });
                push_stack!(finish_json_value(
                    value,
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span
                )?);
            }
            Op::JsonAsIntList => {
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_as_int_list requires Json",
                            instruction.span,
                        ));
                    }
                };
                let items = value.as_array().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_as_int_list requires an array",
                        instruction.span,
                    )
                })?;
                if items.len() > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "JSON integer list exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                let mut values = Vec::with_capacity(items.len());
                let mut allocation = size_of::<Vec<Value>>()
                    .saturating_add(items.len().saturating_mul(size_of::<Value>()));
                for item in items {
                    let integer = json_exact_integer(item).ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "JSON array values must be exact integers",
                            instruction.span,
                        )
                    })?;
                    if integer.digit_count() > self.policy.max_integer_digits {
                        return Err(RuntimeError::new(
                            "R_INTEGER_LIMIT",
                            "JSON integer exceeds the configured digit limit",
                            instruction.span,
                        ));
                    }
                    allocation = allocation
                        .saturating_add(size_of::<Value>())
                        .saturating_add(integer.digit_count());
                    values.push(Value::Int(integer));
                }
                charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                push_stack!(Value::List(Shared::new(values)));
            }
            Op::JsonProjectColumns => {
                let fields = match pop!() {
                    Value::List(fields) => fields
                        .iter()
                        .map(|field| match field {
                            Value::Text(field) => Ok(field.clone()),
                            _ => Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "json_project_columns field names must be Text",
                                instruction.span,
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_project_columns requires a List[Text] of fields",
                            instruction.span,
                        ));
                    }
                };
                let path = match pop!() {
                    Value::List(path) => path
                        .iter()
                        .map(|part| match part {
                            Value::Text(part) => Ok(part.clone()),
                            _ => Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "json_project_columns path segments must be Text",
                                instruction.span,
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_project_columns requires a List[Text] path",
                            instruction.span,
                        ));
                    }
                };
                let source = match pop!() {
                    Value::Text(source) => source,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_project_columns requires JSON Text",
                            instruction.span,
                        ));
                    }
                };
                if source.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "JSON input exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                let path = path.iter().map(|part| part.to_string()).collect();
                let fields = fields.iter().map(|field| field.to_string()).collect();
                let result = json_projection::project_columns(
                    &source,
                    path,
                    fields,
                    self.policy.max_collection_items,
                    self.policy.max_integer_digits,
                );
                match result {
                    Ok(value) => push_stack!(Value::ResultOk(Box::new(finish_json_value(
                        value,
                        &self.policy.vm_limits(),
                        budget.as_ref(),
                        instruction.span,
                    )?))),
                    Err(error) => {
                        push_stack!(Value::ResultErr(Box::new(Value::JsonError(Box::new(
                            error,
                        )))))
                    }
                }
            }
            Op::JsonTextCategoriesValues => {
                let values = match pop!() {
                    Value::Json(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_text_categories_values requires Json values",
                            instruction.span,
                        ));
                    }
                };
                let values = values.as_array().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_text_categories_values requires an array",
                        instruction.span,
                    )
                })?;
                if values.len() > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "JSON category column exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                let mut codes = Vec::with_capacity(values.len());
                let mut labels = Vec::<String>::new();
                let mut category_codes = HashMap::<String, usize>::new();
                for value in values {
                    let label = value.as_str().ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "JSON category values must be Text",
                            instruction.span,
                        )
                    })?;
                    let code = if let Some(code) = category_codes.get(label) {
                        *code
                    } else {
                        let code = labels.len();
                        if code >= self.policy.max_collection_items {
                            return Err(RuntimeError::new(
                                "R_COLLECTION_LIMIT",
                                "JSON categories exceed the configured item limit",
                                instruction.span,
                            ));
                        }
                        charge_allocation(
                            budget.as_ref(),
                            size_of::<(String, usize)>()
                                .saturating_add(label.len().saturating_mul(2)),
                            instruction.span,
                        )?;
                        let label = label.to_owned();
                        category_codes.insert(label.clone(), code);
                        labels.push(label);
                        code
                    };
                    codes.push(serde_json::Value::Number(serde_json::Number::from(
                        code as u64,
                    )));
                }
                let labels = labels
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect::<Vec<_>>();
                let result = serde_json::json!({ "codes": codes, "labels": labels });
                push_stack!(finish_json_value(
                    result,
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span
                )?);
            }
            Op::JsonAsText => match pop!() {
                Value::Json(value) => match value.as_ref() {
                    SharedJson::String(text) => {
                        charge_allocation(
                            budget.as_ref(),
                            size_of::<String>().saturating_add(text.len()),
                            instruction.span,
                        )?;
                        push_stack!(Value::Text(text.to_string().into()))
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_JSON_TYPE",
                            "JSON value is not Text",
                            instruction.span,
                        ));
                    }
                },
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_as_text requires Json",
                        instruction.span,
                    ));
                }
            },
            Op::JsonAsInt => match pop!() {
                Value::Json(value) if value.is_number() => {
                    let integer = json_exact_integer(value.as_ref()).ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "JSON number is not an exact integer",
                            instruction.span,
                        )
                    })?;
                    if integer.digit_count() > self.policy.max_integer_digits {
                        return Err(RuntimeError::new(
                            "R_INTEGER_LIMIT",
                            "JSON integer exceeds the configured digit limit",
                            instruction.span,
                        ));
                    }
                    charge_allocation(
                        budget.as_ref(),
                        size_of::<BigInt>().saturating_add(integer.digit_count()),
                        instruction.span,
                    )?;
                    push_stack!(Value::Int(integer));
                }
                Value::Json(_) => {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "JSON value is not an integer",
                        instruction.span,
                    ));
                }
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_as_int requires Json",
                        instruction.span,
                    ));
                }
            },
            Op::JsonAsFloat => match pop!() {
                Value::Json(value) if value.is_number() => {
                    let number = value
                        .as_number()
                        .and_then(serde_json::Number::as_f64)
                        .ok_or_else(|| {
                            RuntimeError::new(
                                "R_JSON_TYPE",
                                "JSON number cannot be represented as Float64",
                                instruction.span,
                            )
                        })?;
                    let value = Float64::new(number).ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "JSON number is outside finite Float64 range",
                            instruction.span,
                        )
                    })?;
                    push_stack!(Value::Float64(value));
                }
                Value::Json(_) => {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "JSON value is not a number",
                        instruction.span,
                    ));
                }
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_as_float requires Json",
                        instruction.span,
                    ));
                }
            },
            Op::JsonAsBool => match pop!() {
                Value::Json(value) if value.is_boolean() => {
                    push_stack!(Value::Bool(
                        value.as_bool().expect("boolean type was checked")
                    ))
                }
                Value::Json(_) => {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "JSON value is not Bool",
                        instruction.span,
                    ));
                }
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_as_bool requires Json",
                        instruction.span,
                    ));
                }
            },
            Op::JsonIsNull => match pop!() {
                Value::Json(value) => push_stack!(Value::Bool(value.is_null())),
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_is_null requires Json",
                        instruction.span,
                    ));
                }
            },
            ref other => {
                return Err(RuntimeError::new(
                    "R_HOST_UNSUPPORTED",
                    format!("instruction {other:?} is not implemented by the runtime host"),
                    instruction.span,
                ));
            }
        }
        Ok(())
    }
}

impl Instance {
    /// Return diagnostic counters for the latest invocation when profiling is enabled.
    pub fn runtime_profile(&self) -> Option<serde_json::Value> {
        self.runtime_profile_metrics
            .as_ref()
            .map(|profile| profile.to_json())
    }

    pub fn native_function_count(&self) -> usize {
        self.native
            .as_ref()
            .map(NativeBackend::function_count)
            .unwrap_or(0)
    }

    pub fn native_function_names(&self) -> Vec<String> {
        self.native
            .as_ref()
            .map(|native| native.compiled_function_names(&self.artifact))
            .unwrap_or_default()
    }

    pub fn native_calls(&self) -> usize {
        self.native_calls.load(Ordering::Relaxed)
    }

    pub fn native_deoptimizations(&self) -> usize {
        self.native_deoptimizations.load(Ordering::Relaxed)
    }

    /// Native-to-VM side exits grouped by function and bytecode instruction.
    pub fn native_deoptimization_sites(&self) -> serde_json::Value {
        let sites = self
            .native_deoptimization_sites
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        serde_json::Value::Array(
            sites
                .iter()
                .filter_map(|((function_index, instruction_index), count)| {
                    let function = self.artifact.functions.get(*function_index)?;
                    let (operation, line, column) = function
                        .code
                        .get(*instruction_index)
                        .map(|instruction| {
                            (
                                format!("{:?}", instruction.op),
                                instruction.span.line,
                                instruction.span.column,
                            )
                        })
                        .unwrap_or_else(|| ("Unknown".into(), 0, 0));
                    Some(serde_json::json!({
                        "function": function.name,
                        "instruction": instruction_index,
                        "operation": operation,
                        "line": line,
                        "column": column,
                        "count": count,
                    }))
                })
                .collect(),
        )
    }

    pub fn native_compilations(&self) -> usize {
        self.native_compilations.load(Ordering::Relaxed)
    }

    /// Number of native file readers still owned by this instance.
    pub fn open_file_reader_count(&self) -> usize {
        self.readers.open_count()
    }

    /// Number of native filesystem, database, and TOML handles owned by this instance.
    pub fn open_native_resource_count(&self) -> usize {
        self.readers.open_count()
            + self.databases.open_count()
            + self.sockets.open_count()
            + self.toml_documents.open_count()
    }

    fn shared_http_client(&mut self, span: Span) -> Result<Arc<HostHttpClient>, RuntimeError> {
        if self.http.is_none() {
            let client = HostHttpClient::new(self.policy.max_parallelism)
                .map_err(|error| RuntimeError::new("R_HTTP_CLIENT", error, span))?;
            self.http = Some(Arc::new(client));
        }
        self.http
            .as_ref()
            .cloned()
            .ok_or_else(|| RuntimeError::new("R_HTTP_CLIENT", "HTTP client is unavailable", span))
    }

    pub fn native_bytecode_instructions(&self) -> usize {
        self.native
            .as_ref()
            .map_or(0, NativeBackend::bytecode_instruction_count)
    }

    pub fn native_bytecode_blocks(&self) -> usize {
        self.native
            .as_ref()
            .map_or(0, NativeBackend::bytecode_block_count)
    }

    pub fn native_cranelift_ir_blocks(&self) -> usize {
        self.native
            .as_ref()
            .map_or(0, NativeBackend::cranelift_ir_block_count)
    }

    pub fn native_cranelift_ir_instructions(&self) -> usize {
        self.native
            .as_ref()
            .map_or(0, NativeBackend::cranelift_ir_instruction_count)
    }

    pub fn native_compile_profile(&self) -> serde_json::Value {
        self.native.as_ref().map_or_else(
            || NativeBackend::eligibility_profile_json(&self.artifact),
            NativeBackend::compile_profile_json,
        )
    }

    pub fn invoke(&mut self, export: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
        self.invoke_with_cancellation(export, args, CancellationToken::new())
    }

    /// Invoke an export with a cancellation token that may be cloned and cancelled by the host.
    /// The token belongs to this invocation and should not be reused after cancellation.
    pub fn invoke_with_cancellation(
        &mut self,
        export: &str,
        args: Vec<Value>,
        cancellation: CancellationToken,
    ) -> Result<Value, RuntimeError> {
        if self.closed {
            return Err(RuntimeError::new(
                "R_INSTANCE_CLOSED",
                "runtime instance is closed",
                Span::default(),
            ));
        }
        let Some(index) = self
            .artifact
            .functions
            .iter()
            .position(|f| f.name == export)
        else {
            return Err(RuntimeError::new(
                "R_EXPORT_UNKNOWN",
                format!("unknown function `{export}`"),
                Span::default(),
            ));
        };
        self.output.clear();
        self.program_stderr.clear();
        self.host_stderr.clear();
        self.effect_report.clear();
        self.omitted_effects = 0;
        self.worker_threads.clear();
        self.readers.close_all();
        self.databases.close_all();
        self.sockets.close_all();
        self.toml_documents.close_all();
        if args.len() != self.artifact.functions[index].parameters.len() {
            return Err(RuntimeError::new(
                "R_CALL_ARITY",
                format!(
                    "function `{export}` expects {} arguments, got {}",
                    self.artifact.functions[index].parameters.len(),
                    args.len()
                ),
                Span::default(),
            ));
        }
        let runtime_profile = self.runtime_profile_enabled.then(|| {
            let profile = Arc::new(RuntimeProfileCounters::default());
            profile.initialize_function_profiles(
                self.artifact
                    .functions
                    .iter()
                    .map(|function| function.name.clone())
                    .collect(),
            );
            profile.set_sampled_mode(self.runtime_profile_sampled);
            profile
        });
        self.runtime_profile_metrics = runtime_profile.clone();
        let budget = Arc::new(Budget::new(
            self.policy.instruction_limit,
            self.policy.allocation_limit_bytes,
            cancellation,
            runtime_profile.clone(),
        ));
        if self.native_enabled && !self.native_compile_attempted {
            self.native_compile_attempted = true;
            self.native = NativeBackend::compile(
                &self.artifact,
                NativeSymbols {
                    reserve_fuel: native_reserve_fuel as *const () as *const u8,
                    release_fuel: native_release_fuel as *const () as *const u8,
                    poll_cancellation: native_poll_cancellation as *const () as *const u8,
                    make_list: native_make_list as *const () as *const u8,
                    list_set_float_copy: native_list_set_float_copy as *const () as *const u8,
                    list_set_int_copy: native_list_set_int_copy as *const () as *const u8,
                    list_set_bool_copy: native_list_set_bool_copy as *const () as *const u8,
                    list_set_float_local: native_list_set_float_local as *const () as *const u8,
                    list_set_int_local: native_list_set_int_local as *const () as *const u8,
                    list_set_bool_local: native_list_set_bool_local as *const () as *const u8,
                    charge_record: native_charge_record as *const () as *const u8,
                    charge_value_clone: native_charge_value_clone as *const () as *const u8,
                    make_tagged_value: native_make_tagged_value as *const () as *const u8,
                    math: native_math as *const () as *const u8,
                    print: native_print as *const () as *const u8,
                    direct_operations: [
                        native_direct_json_parse as *const () as *const u8,
                        native_direct_json_try_text as *const () as *const u8,
                        native_direct_json_try_get as *const () as *const u8,
                        native_direct_json_try_int as *const () as *const u8,
                        native_direct_file_next_line as *const () as *const u8,
                        native_direct_get_field as *const () as *const u8,
                        native_direct_args as *const () as *const u8,
                        native_direct_text_equal as *const () as *const u8,
                        native_direct_print_err as *const () as *const u8,
                        native_direct_make_typed_map as *const () as *const u8,
                        native_direct_json_array as *const () as *const u8,
                        native_direct_map_keys as *const () as *const u8,
                        native_direct_json_index as *const () as *const u8,
                        native_direct_text_chars as *const () as *const u8,
                        native_direct_json_get as *const () as *const u8,
                        native_direct_json_stringify as *const () as *const u8,
                        native_direct_text_concat as *const () as *const u8,
                        native_direct_json_object as *const () as *const u8,
                        native_direct_map_index as *const () as *const u8,
                        native_direct_map_set as *const () as *const u8,
                        native_direct_json_int as *const () as *const u8,
                        native_direct_map_has as *const () as *const u8,
                        native_direct_json_text as *const () as *const u8,
                        native_direct_file_open_lines as *const () as *const u8,
                        native_direct_file_close as *const () as *const u8,
                        native_direct_text_collapse_whitespace as *const () as *const u8,
                        native_direct_time_parse_timestamp as *const () as *const u8,
                        native_direct_time_format_utc as *const () as *const u8,
                        native_direct_text_slice as *const () as *const u8,
                        native_direct_json_set_local as *const () as *const u8,
                        native_direct_json_push_local as *const () as *const u8,
                        native_direct_text_find as *const () as *const u8,
                        native_direct_text_split as *const () as *const u8,
                        native_direct_text_replace as *const () as *const u8,
                        native_direct_text_trim as *const () as *const u8,
                        native_direct_text_join as *const () as *const u8,
                        native_direct_text_length as *const () as *const u8,
                    ],
                    make_record: native_direct_make_record as *const () as *const u8,
                    capture_vm_continuation: native_capture_vm_continuation as *const ()
                        as *const u8,
                    capture_vm_continuation_fixed: native_capture_vm_continuation_fixed as *const ()
                        as *const u8,
                    capture_vm_continuation_int2: native_capture_vm_continuation_int2 as *const ()
                        as *const u8,
                    count_guest_branch: native_count_guest_branch as *const () as *const u8,
                    loop_safepoint: native_loop_safepoint as *const () as *const u8,
                },
                runtime_profile.is_some(),
                self.native_whole_program_entry.then_some(index),
            );
            if let Some(native) = self.native.as_ref() {
                self.native_code = Some(native.shared_code());
                self.native_compilations.fetch_add(1, Ordering::Relaxed);
            }
        }
        for (value, ty) in args.iter().zip(&self.artifact.functions[index].parameters) {
            if !value_matches(value, ty, &self.artifact.records) {
                return Err(RuntimeError::new(
                    "R_CALL_TYPE",
                    format!("argument type mismatch: expected {ty:?}"),
                    Span::default(),
                ));
            }
            if let Some(error) = value_limit_error_at_boundary(value, &self.policy, Span::default())
            {
                return Err(error);
            }
            charge_allocation(
                budget.as_ref(),
                input_value_allocation(value),
                Span::default(),
            )?;
        }
        let result = self.invoke_inner(index, args, 0, budget);
        self.readers.close_all();
        self.databases.close_all();
        self.sockets.close_all();
        self.toml_documents.close_all();
        result
    }
    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.output)
    }
    pub fn take_program_stderr(&mut self) -> String {
        std::mem::take(&mut self.program_stderr)
    }
    pub fn close(&mut self) {
        self.closed = true;
        if let Some(cancelled) = &self.cancelled {
            cancelled.store(true, Ordering::Relaxed);
        }
        self.readers.close_all();
        self.databases.close_all();
        self.sockets.close_all();
        self.toml_documents.close_all();
        self.http = None;
        self.output.clear();
        self.program_stderr.clear();
        self.host_stderr.clear();
        #[cfg(feature = "python-interop")]
        if let Some(bridge) = &mut self.python_bridge {
            bridge.close();
        }
    }
    pub fn take_host_stderr(&mut self) -> String {
        std::mem::take(&mut self.host_stderr)
    }
    pub fn python_native_image_observation(&self) -> &str {
        &self.python_native_image_observation
    }
    pub fn python_native_images(&self) -> &[String] {
        &self.python_native_images
    }
    pub fn parallel_threads_used(&self) -> usize {
        self.worker_threads.len()
    }

    /// Effect attempts from the most recent invocation and their last observed state.
    pub fn effect_report(&self) -> &[EffectRecord] {
        &self.effect_report
    }

    /// Number of additional effect attempts omitted after the report reached its cap.
    pub fn omitted_effects(&self) -> usize {
        self.omitted_effects
    }

    fn begin_effect(&mut self, kind: EffectKind, target: impl AsRef<str>) -> usize {
        if self.effect_report.len() >= MAX_EFFECT_REPORT_ENTRIES {
            self.omitted_effects = self.omitted_effects.saturating_add(1);
            return usize::MAX;
        }
        let source = target.as_ref();
        let mut target = source
            .chars()
            .take(MAX_EFFECT_TARGET_CHARS)
            .collect::<String>();
        let target_truncated = source.chars().nth(MAX_EFFECT_TARGET_CHARS).is_some();
        if target_truncated {
            target.push('…');
        }
        let index = self.effect_report.len();
        self.effect_report.push(EffectRecord {
            kind,
            target,
            target_truncated,
            outcome: EffectOutcome::Started,
        });
        index
    }

    fn finish_effect(&mut self, index: usize, outcome: EffectOutcome) {
        if index == usize::MAX {
            return;
        }
        if let Some(effect) = self.effect_report.get_mut(index) {
            effect.outcome = outcome;
        }
    }

    #[cfg(feature = "python-interop")]
    fn record_python_native_images(&mut self, observation: &str, images: &[String]) {
        self.python_native_image_observation = observation.to_owned();
        for image in images {
            if !self.python_native_images.contains(image) {
                self.python_native_images.push(image.clone());
            }
        }
    }

    fn invoke_inner(
        &mut self,
        index: usize,
        args: Vec<Value>,
        depth: usize,
        budget: Arc<Budget>,
    ) -> Result<Value, RuntimeError> {
        let allow_native = self
            .native_code
            .as_ref()
            .is_some_and(|native| native.has_function(index));
        match self.start_guest_frame(index, args, depth, budget.clone(), None, allow_native)? {
            FrameStart::Returned(value) => Ok(value),
            FrameStart::Vm(frame) => self.run_guest_frames(frame, budget),
            FrameStart::VmFrames(frames) => self.run_guest_frame_stack(frames, budget),
        }
    }

    fn commit_native_output(&mut self, arena: &mut NativeListArena) {
        let output = std::mem::take(&mut arena.pending_output);
        arena.committed_output_bytes = arena.committed_output_bytes.saturating_add(output.len());
        self.output.push_str(&output);
        for bytes in std::mem::take(&mut arena.pending_print_bytes) {
            let effect = self.begin_effect(EffectKind::ProgramOutput, format!("{bytes} bytes"));
            self.finish_effect(effect, EffectOutcome::Returned);
        }
    }

    fn sort_by_left_before_or_equal(
        &mut self,
        comparator: usize,
        left: &Value,
        right: &Value,
        depth: usize,
        budget: Arc<Budget>,
        span: Span,
    ) -> Result<bool, RuntimeError> {
        let ordering = match self.invoke_inner(
            comparator,
            vec![left.clone(), right.clone()],
            depth + 1,
            budget,
        ) {
            Ok(Value::Int(ordering)) => ordering,
            Ok(_) => {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "sort comparator must return Int",
                    span,
                ));
            }
            Err(mut error) => {
                error.trace.push(span);
                return Err(error);
            }
        };
        Ok(ordering.is_negative() || ordering.is_zero())
    }

    fn start_guest_frame(
        &mut self,
        index: usize,
        args: Vec<Value>,
        depth: usize,
        budget: Arc<Budget>,
        call_site: Option<Span>,
        allow_native: bool,
    ) -> Result<FrameStart, RuntimeError> {
        let artifact = Arc::clone(&self.artifact);
        let f = &artifact.functions[index];
        check_call_depth(&artifact, &self.policy.vm_limits(), index, depth)?;
        // Entering native code copies every list argument into a fresh arena. A small
        // function called from a VM loop with the same large list would pay that copy on
        // every call, so once its entries are shown to be dominated by the copy, keep it in
        // the VM. Results are identical in both tiers; only the cost changes.
        let entry_list_items = args
            .iter()
            .map(|value| match value {
                Value::List(items) => items.len(),
                Value::Float64List(items) => items.len(),
                _ => 0,
            })
            .sum::<usize>();
        let large_entry = entry_list_items >= LIST_ITEMS_TRACKED_AT_ENTRY;
        if !allow_native
            || (large_entry
                && self
                    .native_entry_copy_dominated
                    .get(index)
                    .copied()
                    .unwrap_or(0)
                    >= COPY_DOMINATED_ENTRY_LIMIT)
        {
            return Ok(FrameStart::Vm(new_vm_frame(
                &artifact, index, args, depth, call_site,
            )));
        }
        let mut native_arena = NativeListArena::new(
            Arc::as_ptr(&budget),
            self.policy.max_collection_items,
            self.policy.output_limit_bytes,
            self.output.len().saturating_add(self.program_stderr.len()),
            budget.runtime_profile.clone(),
        );
        native_arena.attach_runtime_context(
            self as *mut Instance,
            Arc::clone(&artifact),
            Arc::clone(&self.native_constants),
            budget.clone(),
            self.policy.vm_limits(),
        );
        let mut native_record_fields: Vec<Box<[i64]>> = Vec::new();
        let mut native_records: Vec<Box<NativeRecord>> = Vec::new();
        let native_args = args
            .iter()
            .zip(&f.parameters)
            .map(|(value, ty)| match (value, ty) {
                (Value::Int(integer), Type::Int) => integer.to_i64(),
                (Value::Float64(float), Type::Float64) => Some(float.get().to_bits() as i64),
                (Value::Float64List(items), Type::List(item)) if **item == Type::Float64 => {
                    let storage_id = native_arena.float_storage.len();
                    native_arena.float_storage.push(items.as_ref().clone());
                    native_arena.float_lists.push(Box::new(NativeFloatList {
                        data: native_arena
                            .float_storage
                            .last_mut()?
                            .as_mut_ptr()
                            .cast::<u64>(),
                        len: items.len(),
                        storage_id,
                        owned: false,
                        inline_storage: false,
                        inline: [0; NATIVE_INLINE_LIST_ITEMS],
                    }));
                    Some((&**native_arena.float_lists.last()? as *const NativeFloatList) as i64)
                }
                (Value::List(items), Type::List(item)) if **item == Type::Float64 => {
                    let values: Vec<f64> = items
                        .iter()
                        .map(|value| match value {
                            Value::Float64(value) => Some(value.get()),
                            _ => None,
                        })
                        .collect::<Option<_>>()?;
                    let storage_id = native_arena.float_storage.len();
                    native_arena.float_storage.push(
                        values
                            .into_iter()
                            .map(Float64::new)
                            .collect::<Option<Vec<_>>>()?,
                    );
                    native_arena.float_lists.push(Box::new(NativeFloatList {
                        data: native_arena
                            .float_storage
                            .last_mut()?
                            .as_mut_ptr()
                            .cast::<u64>(),
                        len: items.len(),
                        storage_id,
                        owned: false,
                        inline_storage: false,
                        inline: [0; NATIVE_INLINE_LIST_ITEMS],
                    }));
                    Some((&**native_arena.float_lists.last()? as *const NativeFloatList) as i64)
                }
                (Value::List(items), Type::List(item)) if **item == Type::Int => {
                    let values: Vec<i64> = items
                        .iter()
                        .map(|value| match value {
                            Value::Int(value) => value.to_i64(),
                            _ => None,
                        })
                        .collect::<Option<_>>()?;
                    let storage_id = native_arena.int_storage.len();
                    native_arena.int_storage.push(values);
                    native_arena.int_lists.push(Box::new(NativeIntList {
                        data: native_arena.int_storage.last_mut()?.as_mut_ptr(),
                        len: items.len(),
                        storage_id,
                        owned: false,
                        inline_storage: false,
                        inline: [0; NATIVE_INLINE_LIST_ITEMS],
                    }));
                    Some((&**native_arena.int_lists.last()? as *const NativeIntList) as i64)
                }
                (Value::List(items), Type::List(item)) if **item == Type::Bool => {
                    let values: Vec<i64> = items
                        .iter()
                        .map(|value| match value {
                            Value::Bool(value) => Some(i64::from(*value)),
                            _ => None,
                        })
                        .collect::<Option<_>>()?;
                    let storage_id = native_arena.int_storage.len();
                    native_arena.int_storage.push(values);
                    native_arena.int_lists.push(Box::new(NativeIntList {
                        data: native_arena.int_storage.last_mut()?.as_mut_ptr(),
                        len: items.len(),
                        storage_id,
                        owned: false,
                        inline_storage: false,
                        inline: [0; NATIVE_INLINE_LIST_ITEMS],
                    }));
                    Some((&**native_arena.int_lists.last()? as *const NativeIntList) as i64)
                }
                (Value::Record(name, fields), Type::Record(expected)) if name == expected => {
                    let definition = self
                        .artifact
                        .records
                        .iter()
                        .find(|record| record.name == *name)?;
                    if fields.len() != definition.fields.len() {
                        return None;
                    }
                    let Some(words) = native_record_words(definition, &self.artifact.records)
                    else {
                        return native_arena.store_value(value.clone());
                    };
                    let mut encoded_fields = Vec::with_capacity(words);
                    for (value, field) in fields.iter().zip(&definition.fields) {
                        if let Some(nested) =
                            native_inline_record(&field.ty, &self.artifact.records)
                        {
                            encode_inline_native_record(
                                value,
                                nested,
                                &self.artifact.records,
                                &mut encoded_fields,
                            )?;
                            continue;
                        }
                        let encoded = match (value, &field.ty) {
                            (Value::Int(integer), Type::Int) => integer.to_i64()?,
                            (Value::Float64(float), Type::Float64) => float.get().to_bits() as i64,
                            (Value::Bool(value), Type::Bool) => i64::from(*value),
                            (Value::Float64List(items), Type::List(item))
                                if **item == Type::Float64 =>
                            {
                                let storage_id = native_arena.float_storage.len();
                                native_arena.float_storage.push(items.as_ref().clone());
                                native_arena.float_lists.push(Box::new(NativeFloatList {
                                    data: native_arena
                                        .float_storage
                                        .last_mut()?
                                        .as_mut_ptr()
                                        .cast::<u64>(),
                                    len: items.len(),
                                    storage_id,
                                    owned: false,
                                    inline_storage: false,
                                    inline: [0; NATIVE_INLINE_LIST_ITEMS],
                                }));
                                (&**native_arena.float_lists.last()? as *const NativeFloatList)
                                    as i64
                            }
                            (Value::List(items), Type::List(item)) if **item == Type::Float64 => {
                                let values: Vec<f64> = items
                                    .iter()
                                    .map(|value| match value {
                                        Value::Float64(value) => Some(value.get()),
                                        _ => None,
                                    })
                                    .collect::<Option<_>>()?;
                                let storage_id = native_arena.float_storage.len();
                                native_arena.float_storage.push(
                                    values
                                        .into_iter()
                                        .map(Float64::new)
                                        .collect::<Option<Vec<_>>>()?,
                                );
                                native_arena.float_lists.push(Box::new(NativeFloatList {
                                    data: native_arena
                                        .float_storage
                                        .last_mut()?
                                        .as_mut_ptr()
                                        .cast::<u64>(),
                                    len: items.len(),
                                    storage_id,
                                    owned: false,
                                    inline_storage: false,
                                    inline: [0; NATIVE_INLINE_LIST_ITEMS],
                                }));
                                (&**native_arena.float_lists.last()? as *const NativeFloatList)
                                    as i64
                            }
                            (Value::List(items), Type::List(item)) if **item == Type::Int => {
                                let values: Vec<i64> = items
                                    .iter()
                                    .map(|value| match value {
                                        Value::Int(value) => value.to_i64(),
                                        _ => None,
                                    })
                                    .collect::<Option<_>>()?;
                                let storage_id = native_arena.int_storage.len();
                                native_arena.int_storage.push(values);
                                native_arena.int_lists.push(Box::new(NativeIntList {
                                    data: native_arena.int_storage.last_mut()?.as_mut_ptr(),
                                    len: items.len(),
                                    storage_id,
                                    owned: false,
                                    inline_storage: false,
                                    inline: [0; NATIVE_INLINE_LIST_ITEMS],
                                }));
                                (&**native_arena.int_lists.last()? as *const NativeIntList) as i64
                            }
                            (Value::List(items), Type::List(item)) if **item == Type::Bool => {
                                let values: Vec<i64> = items
                                    .iter()
                                    .map(|value| match value {
                                        Value::Bool(value) => Some(i64::from(*value)),
                                        _ => None,
                                    })
                                    .collect::<Option<_>>()?;
                                let storage_id = native_arena.int_storage.len();
                                native_arena.int_storage.push(values);
                                native_arena.int_lists.push(Box::new(NativeIntList {
                                    data: native_arena.int_storage.last_mut()?.as_mut_ptr(),
                                    len: items.len(),
                                    storage_id,
                                    owned: false,
                                    inline_storage: false,
                                    inline: [0; NATIVE_INLINE_LIST_ITEMS],
                                }));
                                (&**native_arena.int_lists.last()? as *const NativeIntList) as i64
                            }
                            _ => return None,
                        };
                        encoded_fields.push(encoded);
                    }
                    let fields = encoded_fields.into_boxed_slice();
                    let descriptor = Box::new(NativeRecord {
                        fields: fields.as_ptr(),
                        len: fields.len(),
                    });
                    native_record_fields.push(fields);
                    native_records.push(descriptor);
                    Some((&**native_records.last()? as *const NativeRecord) as i64)
                }
                (Value::OptionSome(_) | Value::OptionNone, Type::Option(_))
                | (Value::ResultOk(_) | Value::ResultErr(_), Type::Result(_, _)) => {
                    native_arena.encode_native_value(value.clone())
                }
                // Native list indexing dereferences a list descriptor, so a plain
                // `values` index here would be read as an address. Text lists get a
                // real handle list (the caller already owns and paid for the value);
                // other list shapes have no entry encoding and stay in the VM.
                (Value::List(items), Type::List(item)) if **item == Type::Text => {
                    let texts = items
                        .iter()
                        .map(|value| match value {
                            Value::Text(text) => Some(SharedText::clone(text)),
                            _ => None,
                        })
                        .collect::<Option<Vec<_>>>()?;
                    native_arena.store_native_text_list_charged(texts, false)
                }
                (_, Type::List(_)) => None,
                _ => native_arena.store_value(value.clone()),
            })
            .collect::<Option<Vec<_>>>();
        if let Some(native_args) = native_args {
            let native_code = self.native_code.clone();
            let call_depth_limit = self.policy.call_depth_limit;
            let fuel_before = budget.remaining.load(Ordering::Relaxed);
            if let Some(mut outcome) = native_code.as_ref().and_then(|native| {
                native.invoke(
                    index,
                    &native_args,
                    Arc::as_ptr(&budget) as *const std::ffi::c_void,
                    depth,
                    call_depth_limit,
                    &mut native_arena,
                )
            }) {
                self.native_calls
                    .fetch_add(outcome.calls, Ordering::Relaxed);
                if large_entry {
                    let fuel_used =
                        fuel_before.saturating_sub(budget.remaining.load(Ordering::Relaxed));
                    if usize::try_from(fuel_used)
                        .is_ok_and(|used| used.saturating_mul(4) < entry_list_items)
                    {
                        if self.native_entry_copy_dominated.len() <= index {
                            self.native_entry_copy_dominated
                                .resize(artifact.functions.len(), 0);
                        }
                        let count = &mut self.native_entry_copy_dominated[index];
                        *count = count.saturating_add(1);
                    }
                }
                // A native failure ends the program, so what it printed first must still reach
                // the output, as it does in the VM. A plain deoptimization replays instead.
                if outcome.status >= 2 || native_arena.host_error.is_some() {
                    self.commit_native_output(&mut native_arena);
                }
                let fault_frame = self
                    .artifact
                    .functions
                    .get(outcome.fault_function)
                    .unwrap_or(f);
                match outcome.status {
                    0 => {
                        let value = match &f.result {
                            Type::Int => Value::Int(BigInt::from_i64(outcome.value)),
                            Type::Float64 => Value::Float64(
                                Float64::new(f64::from_bits(outcome.value as u64)).unwrap(),
                            ),
                            Type::Bool => Value::Bool(outcome.value != 0),
                            Type::Unit => Value::Unit,
                            Type::Record(name)
                                if self
                                    .artifact
                                    .records
                                    .iter()
                                    .find(|record| record.name == *name)
                                    .is_some_and(|record| {
                                        native_record_has_native_layout(
                                            record,
                                            &self.artifact.records,
                                        )
                                    }) =>
                            {
                                let definition = self
                                    .artifact
                                    .records
                                    .iter()
                                    .find(|record| record.name == *name)
                                    .expect("native record result has a verified definition");
                                let mut fields = Vec::with_capacity(definition.fields.len());
                                let mut decoded_float_lists =
                                    std::collections::HashMap::<usize, Shared<Vec<Float64>>>::new();
                                let mut decoded_int_lists =
                                    std::collections::HashMap::<usize, Shared<Vec<Value>>>::new();
                                let mut decoded_bool_lists =
                                    std::collections::HashMap::<usize, Shared<Vec<Value>>>::new();
                                let records = &self.artifact.records;
                                let mut word = 0_usize;
                                for field_type in definition.fields.iter().map(|field| &field.ty) {
                                    if let Some(nested) = native_inline_record(field_type, records)
                                    {
                                        let words = native_record_words(nested, records)
                                            .unwrap_or(usize::MAX);
                                        let value = outcome
                                            .result_values
                                            .get(word..word.saturating_add(words))
                                            .and_then(|encoded| {
                                                decode_inline_native_record(encoded, nested, records)
                                            })
                                            .ok_or_else(|| {
                                                RuntimeError::new(
                                                    "R_NATIVE_RESULT",
                                                    "native record returned an invalid nested record",
                                                    f.code
                                                        .last()
                                                        .map(|instruction| instruction.span)
                                                        .unwrap_or_default(),
                                                )
                                            })?;
                                        word = word.saturating_add(words);
                                        fields.push(value);
                                        continue;
                                    }
                                    let Some(encoded) = outcome.result_values.get(word) else {
                                        unreachable!(
                                            "native result words are sized at compile time"
                                        )
                                    };
                                    word += 1;
                                    let field = match field_type {
                                        Type::Int => {
                                            Value::Int(BigInt::parse(&encoded.to_string()).unwrap())
                                        }
                                        Type::Float64 => Value::Float64(
                                            Float64::new(f64::from_bits(*encoded as u64)).unwrap(),
                                        ),
                                        Type::Bool => Value::Bool(*encoded != 0),
                                        Type::List(item) if **item == Type::Float64 => {
                                            let descriptor =
                                                *encoded as usize as *const NativeFloatList;
                                            if descriptor.is_null() {
                                                return Err(RuntimeError::new(
                                                    "R_NATIVE_RESULT",
                                                    "native record returned a null list descriptor",
                                                    f.code
                                                        .last()
                                                        .map(|instruction| instruction.span)
                                                        .unwrap_or_default(),
                                                ));
                                            }
                                            let list_key = descriptor as usize;
                                            let values: Shared<Vec<Float64>> = if let Some(values) =
                                                decoded_float_lists.get(&list_key)
                                            {
                                                values.clone()
                                            } else {
                                                let descriptor = unsafe { &*descriptor };
                                                let values: Shared<Vec<Float64>> = if descriptor
                                                    .inline_storage
                                                {
                                                    let encoded = unsafe {
                                                        std::slice::from_raw_parts(
                                                            descriptor.data,
                                                            descriptor.len,
                                                        )
                                                    };
                                                    Shared::new(
                                                        encoded
                                                            .iter()
                                                            .map(|bits| {
                                                                Float64::new(f64::from_bits(*bits))
                                                                    .unwrap()
                                                            })
                                                            .collect(),
                                                    )
                                                } else {
                                                    let Some(storage) = native_arena
                                                        .float_storage
                                                        .get_mut(descriptor.storage_id)
                                                    else {
                                                        return Err(RuntimeError::new(
                                                            "R_NATIVE_RESULT",
                                                            "native record returned an invalid list storage handle",
                                                            f.code
                                                                .last()
                                                                .map(|instruction| instruction.span)
                                                                .unwrap_or_default(),
                                                        ));
                                                    };
                                                    Shared::new(std::mem::take(storage))
                                                };
                                                decoded_float_lists
                                                    .insert(list_key, values.clone());
                                                values
                                            };
                                            Value::Float64List(values)
                                        }
                                        Type::List(item) if **item == Type::Int => {
                                            let descriptor =
                                                *encoded as usize as *const NativeIntList;
                                            if descriptor.is_null() {
                                                return Err(RuntimeError::new(
                                                    "R_NATIVE_RESULT",
                                                    "native record returned a null integer-list descriptor",
                                                    f.code
                                                        .last()
                                                        .map(|instruction| instruction.span)
                                                        .unwrap_or_default(),
                                                ));
                                            }
                                            let list_key = descriptor as usize;
                                            let values: Shared<Vec<Value>> = if let Some(values) =
                                                decoded_int_lists.get(&list_key)
                                            {
                                                values.clone()
                                            } else {
                                                let descriptor = unsafe { &*descriptor };
                                                let values: Shared<Vec<Value>> = if descriptor
                                                    .inline_storage
                                                {
                                                    let encoded = unsafe {
                                                        std::slice::from_raw_parts(
                                                            descriptor.data,
                                                            descriptor.len,
                                                        )
                                                    };
                                                    Shared::new(
                                                        encoded
                                                            .iter()
                                                            .map(|value| {
                                                                Value::Int(
                                                                    BigInt::parse(
                                                                        &value.to_string(),
                                                                    )
                                                                    .unwrap(),
                                                                )
                                                            })
                                                            .collect(),
                                                    )
                                                } else {
                                                    let Some(storage) = native_arena
                                                        .int_storage
                                                        .get_mut(descriptor.storage_id)
                                                    else {
                                                        return Err(RuntimeError::new(
                                                            "R_NATIVE_RESULT",
                                                            "native record returned an invalid integer-list storage handle",
                                                            f.code
                                                                .last()
                                                                .map(|instruction| instruction.span)
                                                                .unwrap_or_default(),
                                                        ));
                                                    };
                                                    Shared::new(
                                                        std::mem::take(storage)
                                                            .into_iter()
                                                            .map(|value| {
                                                                Value::Int(
                                                                    BigInt::parse(
                                                                        &value.to_string(),
                                                                    )
                                                                    .unwrap(),
                                                                )
                                                            })
                                                            .collect(),
                                                    )
                                                };
                                                decoded_int_lists.insert(list_key, values.clone());
                                                values
                                            };
                                            Value::List(values)
                                        }
                                        Type::List(item) if **item == Type::Bool => {
                                            let descriptor =
                                                *encoded as usize as *const NativeIntList;
                                            if descriptor.is_null() {
                                                return Err(RuntimeError::new(
                                                    "R_NATIVE_RESULT",
                                                    "native record returned a null boolean-list descriptor",
                                                    f.code
                                                        .last()
                                                        .map(|instruction| instruction.span)
                                                        .unwrap_or_default(),
                                                ));
                                            }
                                            let list_key = descriptor as usize;
                                            let values: Shared<Vec<Value>> = if let Some(values) =
                                                decoded_bool_lists.get(&list_key)
                                            {
                                                values.clone()
                                            } else {
                                                let descriptor = unsafe { &*descriptor };
                                                let values: Shared<Vec<Value>> = if descriptor
                                                    .inline_storage
                                                {
                                                    let encoded = unsafe {
                                                        std::slice::from_raw_parts(
                                                            descriptor.data,
                                                            descriptor.len,
                                                        )
                                                    };
                                                    Shared::new(
                                                        encoded
                                                            .iter()
                                                            .map(|value| Value::Bool(*value != 0))
                                                            .collect(),
                                                    )
                                                } else {
                                                    let Some(storage) = native_arena
                                                        .int_storage
                                                        .get_mut(descriptor.storage_id)
                                                    else {
                                                        return Err(RuntimeError::new(
                                                            "R_NATIVE_RESULT",
                                                            "native record returned an invalid boolean-list storage handle",
                                                            f.code
                                                                .last()
                                                                .map(|instruction| instruction.span)
                                                                .unwrap_or_default(),
                                                        ));
                                                    };
                                                    Shared::new(
                                                        std::mem::take(storage)
                                                            .into_iter()
                                                            .map(|value| Value::Bool(value != 0))
                                                            .collect(),
                                                    )
                                                };
                                                decoded_bool_lists.insert(list_key, values.clone());
                                                values
                                            };
                                            Value::List(values)
                                        }
                                        _ => unreachable!(
                                            "native result fields are checked at compile time"
                                        ),
                                    };
                                    fields.push(field);
                                }
                                Value::Record(name.as_str().into(), Shared::new(fields))
                            }
                            _ => native_arena
                                .materialize_native_value(outcome.value)
                                .ok_or_else(|| {
                                    RuntimeError::new(
                                        "R_NATIVE_RESULT",
                                        "native function returned an invalid value handle",
                                        f.code
                                            .last()
                                            .map(|instruction| instruction.span)
                                            .unwrap_or_default(),
                                    )
                                })?,
                        };
                        self.commit_native_output(&mut native_arena);
                        return Ok(FrameStart::Returned(value));
                    }
                    1 => {
                        if let Some(error) = native_arena.host_error.take() {
                            return Err(error);
                        }
                        self.native_deoptimizations.fetch_add(1, Ordering::Relaxed);
                        *self
                            .native_deoptimization_sites
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .entry((outcome.fault_function, outcome.fault_ip))
                            .or_default() += 1;
                        if !outcome.continuations.is_empty() {
                            self.commit_native_output(&mut native_arena);
                            if let Some(deepest) = outcome.continuations.last_mut() {
                                deepest.call_site = call_site;
                            }
                            let frames = outcome
                                .continuations
                                .into_iter()
                                .rev()
                                .map(|frame| VmFrame {
                                    function_index: frame.function_index,
                                    depth: frame.depth,
                                    call_site: frame.call_site,
                                    locals: frame.locals,
                                    stack: frame.stack,
                                    ip: frame.ip,
                                })
                                .collect();
                            return Ok(FrameStart::VmFrames(frames));
                        }
                        budget.refund(outcome.steps);
                        budget.refund_allocation(outcome.allocations);
                    }
                    2 => {
                        let span = fault_frame
                            .code
                            .get(outcome.fault_ip)
                            .map(|i| i.span)
                            .unwrap_or_default();
                        return Err(native_runtime_error(
                            &self.artifact,
                            &outcome.trace,
                            "R_DIV_ZERO",
                            "integer division by zero",
                            span,
                        ));
                    }
                    3 => {
                        let span = fault_frame
                            .code
                            .get(outcome.fault_ip)
                            .map(|i| i.span)
                            .unwrap_or_default();
                        return Err(native_runtime_error(
                            &self.artifact,
                            &outcome.trace,
                            "R_STEP_LIMIT",
                            "instruction budget exhausted",
                            span,
                        ));
                    }
                    4 => {
                        let span = fault_frame
                            .code
                            .get(outcome.fault_ip)
                            .map(|i| i.span)
                            .unwrap_or_default();
                        return Err(native_runtime_error(
                            &self.artifact,
                            &outcome.trace,
                            "R_FLOAT_NONFINITE",
                            "Float64 operation produced a non-finite value",
                            span,
                        ));
                    }
                    5 => {
                        let span = fault_frame.code.first().map(|i| i.span).unwrap_or_default();
                        return Err(native_runtime_error(
                            &self.artifact,
                            &outcome.trace,
                            "R_CALL_DEPTH",
                            format!("call depth exceeds {}", self.policy.call_depth_limit),
                            span,
                        ));
                    }
                    6 => {
                        let span = fault_frame
                            .code
                            .get(outcome.fault_ip)
                            .map(|i| i.span)
                            .unwrap_or_default();
                        return Err(native_runtime_error(
                            &self.artifact,
                            &outcome.trace,
                            "R_CANCELLED",
                            "invocation was cancelled",
                            span,
                        ));
                    }
                    7 => {
                        let span = fault_frame
                            .code
                            .get(outcome.fault_ip)
                            .map(|i| i.span)
                            .unwrap_or_default();
                        return Err(native_runtime_error(
                            &self.artifact,
                            &outcome.trace,
                            "R_MATH_DOMAIN",
                            "sqrt requires a non-negative value",
                            span,
                        ));
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_NATIVE_INVALID",
                            "native function returned an invalid status",
                            f.code.first().map(|i| i.span).unwrap_or_default(),
                        ));
                    }
                }
            }
        }
        Ok(FrameStart::Vm(new_vm_frame(
            &artifact, index, args, depth, call_site,
        )))
    }

    fn run_guest_frames(
        &mut self,
        initial: VmFrame,
        budget: Arc<Budget>,
    ) -> Result<Value, RuntimeError> {
        guion_vm_core::run_frames(self, initial, budget)
    }

    fn run_guest_frame_stack(
        &mut self,
        frames: Vec<VmFrame>,
        budget: Arc<Budget>,
    ) -> Result<Value, RuntimeError> {
        guion_vm_core::run_frames_with_stack(self, frames, budget)
    }

    fn call_python(
        &mut self,
        target: usize,
        args: &[Value],
        kwargs: &[(String, Value)],
        budget: &Budget,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        #[cfg(not(feature = "python-interop"))]
        {
            let _ = (target, args, kwargs, budget);
            Err(RuntimeError::new(
                "R_PYTHON_UNAVAILABLE",
                "this external call requires the optional `python-interop` feature",
                span,
            ))
        }
        #[cfg(feature = "python-interop")]
        {
            let external = self
                .artifact
                .externals
                .get(target)
                .cloned()
                .ok_or_else(|| {
                    RuntimeError::new(
                        "R_ARTIFACT_INVALID",
                        "Python call references a missing declaration",
                        span,
                    )
                })?;
            let interpreter = self.policy.python_interpreter.clone().ok_or_else(|| {
            RuntimeError::new(
                "R_PYTHON_UNCONFIGURED",
                "this project declares a Python call; configure an explicit interpreter or install the managed bridge",
                span,
            )
        })?;
            if self.python_bridge.is_none() {
                let project_root = self
                    .policy
                    .file_root
                    .as_deref()
                    .unwrap_or_else(|| std::path::Path::new("."));
                let working_directory = self
                    .policy
                    .additional_file_roots
                    .last()
                    .map(std::path::PathBuf::as_path)
                    .unwrap_or(project_root);
                self.python_bridge = Some(
                    python_bridge::PythonBridge::launch_with_project_root(
                        &interpreter,
                        project_root,
                        working_directory,
                        self.policy.python_timeout,
                    )
                    .map_err(|error| RuntimeError::new(error.code, error.message, span))?,
                );
            }
            let effect = self.begin_effect(
                EffectKind::PythonCall,
                format!("{}:{}", external.module, external.callable),
            );
            let call = self.python_bridge.as_mut().unwrap().call_with_cancellation(
                &external.module,
                &external.callable,
                args,
                kwargs,
                &external.result,
                &budget.cancellation,
            );
            self.finish_effect(
                effect,
                match &call {
                    Ok(_) => EffectOutcome::Returned,
                    Err(error) if error.code == "R_CANCELLED" => EffectOutcome::Interrupted,
                    Err(_) => EffectOutcome::ReturnedError,
                },
            );
            if let Some(bridge) = &self.python_bridge {
                let observation = bridge.native_image_observation().to_owned();
                let images = bridge.native_images().to_vec();
                self.record_python_native_images(&observation, &images);
            }
            match call {
                Ok(result) => {
                    charge_allocation(
                        budget,
                        input_value_allocation(&result.value)
                            .saturating_add(result.stdout.len())
                            .saturating_add(result.stderr.len()),
                        span,
                    )?;
                    if self.output.len().saturating_add(result.stdout.len())
                        > self.policy.output_limit_bytes
                    {
                        return Err(RuntimeError::new(
                            "R_OUTPUT_LIMIT",
                            "Python output exceeds the configured limit",
                            span,
                        ));
                    }
                    self.output.push_str(&result.stdout);
                    if self.host_stderr.len().saturating_add(result.stderr.len())
                        > self.policy.output_limit_bytes
                    {
                        return Err(RuntimeError::new(
                            "R_OUTPUT_LIMIT",
                            "Python stderr exceeds the configured limit",
                            span,
                        ));
                    }
                    self.host_stderr.push_str(&result.stderr);
                    if let Some(error) =
                        value_limit_error_at_boundary(&result.value, &self.policy, span)
                    {
                        return Err(error);
                    }
                    Ok(result.value)
                }
                Err(error) => {
                    if self.output.len().saturating_add(error.stdout.len())
                        <= self.policy.output_limit_bytes
                    {
                        self.output.push_str(&error.stdout);
                    }
                    if self.host_stderr.len().saturating_add(error.stderr.len())
                        <= self.policy.output_limit_bytes
                    {
                        self.host_stderr.push_str(&error.stderr);
                    }
                    let detail = error
                        .python_type
                        .map(|kind| format!("Python {kind}: "))
                        .unwrap_or_default();
                    let trace = error
                        .traceback
                        .map(|trace| format!("\n{trace}"))
                        .unwrap_or_default();
                    Err(RuntimeError::new(
                        error.code,
                        format!("{detail}{}{trace}", error.message),
                        span,
                    ))
                }
            }
        }
    }

    fn parallel_map(
        &mut self,
        function: usize,
        values: Vec<Value>,
        limit: usize,
        depth: usize,
        budget: Arc<Budget>,
        span: Span,
    ) -> Result<Result<Vec<Value>, TaskErrorValue>, RuntimeError> {
        if values.len() > self.policy.max_collection_items {
            return Err(RuntimeError::new(
                "R_COLLECTION_LIMIT",
                "parallel input exceeds the configured item limit",
                span,
            ));
        }
        if values.is_empty() {
            return Ok(Ok(Vec::new()));
        }
        if function >= self.artifact.functions.len() {
            return Err(RuntimeError::new(
                "R_ARTIFACT_INVALID",
                "parallel_map references a missing function",
                span,
            ));
        }
        let expected = self.artifact.functions[function]
            .parameters
            .first()
            .ok_or_else(|| {
                RuntimeError::new(
                    "R_ARTIFACT_INVALID",
                    "parallel worker must accept one argument",
                    span,
                )
            })?;
        if self.artifact.functions[function].parameters.len() != 1
            || values
                .iter()
                .any(|value| !value_matches(value, expected, &self.artifact.records))
        {
            return Err(RuntimeError::new(
                "R_CALL_TYPE",
                "parallel_map input does not match worker parameter type",
                span,
            ));
        }
        let worker_count = limit.min(values.len());
        let worker_input_copies = values
            .iter()
            .map(cloned_value_allocation)
            .fold(0usize, usize::saturating_add);
        charge_allocation(budget.as_ref(), worker_input_copies, span)?;
        charge_allocation(
            budget.as_ref(),
            size_of::<Vec<Value>>()
                .saturating_add(values.len().saturating_mul(size_of::<Value>()))
                .saturating_add(
                    values
                        .len()
                        .saturating_mul(size_of::<Option<(Result<Value, RuntimeError>, String)>>()),
                ),
            span,
        )?;
        // Workers read the inputs and the artifact's constants, so their reference counts
        // must be atomic before any worker starts.
        values.share_across_threads();
        self.artifact.share_across_threads();
        self.registers.share_across_threads();
        let next = Arc::new(AtomicUsize::new(worker_count));
        let cancelled = Arc::new(AtomicBool::new(false));
        let artifact = Arc::clone(&self.artifact);
        let registers = Arc::clone(&self.registers);
        let native_constants = Arc::clone(&self.native_constants);
        let policy = self.policy.clone();
        let native_enabled = self.native_enabled;
        let native_code = self.native_code.clone();
        let values = Arc::new(values);
        let native_calls = self.native_calls.clone();
        let native_deoptimizations = self.native_deoptimizations.clone();
        let native_deoptimization_sites = self.native_deoptimization_sites.clone();
        let native_compilations = self.native_compilations.clone();
        // Reserve most of the remaining global instruction allowance for workers,
        // while keeping enough for the caller to inspect and handle the Result.
        // The workers share one bounded child budget, so the total remains bounded.
        let worker_budget = Arc::new(Budget {
            remaining: AtomicU64::new(budget.reserve_parallel()),
            allocation: budget.allocation.clone(),
            allocation_floor: budget.parallel_allocation_floor(),
            cancellation: budget.cancellation.clone(),
            local_cancellation: Some(cancelled.clone()),
            runtime_profile: budget.runtime_profile.clone(),
        });
        // Workers draw from the same allocation pool, so it is updated atomically until
        // every worker has been joined.
        budget.allocation.begin_sharing();
        let (sender, receiver) = mpsc::channel();
        let mut handles = Vec::new();
        for initial_index in 0..worker_count {
            let next = next.clone();
            let cancelled = cancelled.clone();
            let artifact = artifact.clone();
            let registers = registers.clone();
            let native_constants = native_constants.clone();
            let policy = policy.clone();
            let values = values.clone();
            let budget = worker_budget.clone();
            let native_calls = native_calls.clone();
            let native_deoptimizations = native_deoptimizations.clone();
            let native_deoptimization_sites = native_deoptimization_sites.clone();
            let native_compilations = native_compilations.clone();
            let native_code = native_code.clone();
            let http = self.http.clone();
            let sockets = self.sockets.share();
            let sender = sender.clone();
            handles.push(thread::spawn(move || {
                let mut assigned_index = Some(initial_index);
                loop {
                    if cancelled.load(Ordering::Relaxed) {
                        break;
                    }
                    let index = assigned_index
                        .take()
                        .unwrap_or_else(|| next.fetch_add(1, Ordering::Relaxed));
                    if index >= values.len() {
                        break;
                    }
                    let thread_id = format!("{:?}", thread::current().id());
                    let mut database_roots = Vec::with_capacity(
                        1 + policy.additional_file_roots.len() + policy.file_root_aliases.len(),
                    );
                    if let Some(root) = &policy.file_root {
                        database_roots.push(root.clone());
                    }
                    database_roots.extend(policy.additional_file_roots.iter().cloned());
                    database_roots.extend(policy.file_root_aliases.values().cloned());
                    let mut worker = Instance {
                        artifact: Arc::clone(&artifact),
                        native_constants: Arc::clone(&native_constants),
                        registers: Arc::clone(&registers),
                        native: None,
                        native_code: native_code.clone(),
                        native_enabled,
                        native_whole_program_entry: false,
                        native_compile_attempted: false,
                        runtime_profile_enabled: false,
                        runtime_profile_sampled: false,
                        runtime_profile_metrics: None,
                        native_calls: native_calls.clone(),
                        native_deoptimizations: native_deoptimizations.clone(),
                        native_deoptimization_sites: native_deoptimization_sites.clone(),
                        native_entry_copy_dominated: Vec::new(),
                        native_compilations: native_compilations.clone(),
                        policy: policy.clone(),
                        http: http.clone(),
                        readers: stdlib_data::FileReaderRegistry::new(
                            usize::MAX,
                            policy.max_file_bytes,
                        ),
                        sockets: sockets.share(),
                        databases: stdlib_services::DatabaseRegistry::with_roots(
                            database_roots,
                            policy.max_file_bytes,
                            policy.max_collection_items,
                            2_000,
                        ),
                        toml_documents: stdlib_services::TomlRegistry::new(policy.max_text_bytes),
                        output: String::new(),
                        program_stderr: String::new(),
                        closed: false,
                        worker_threads: HashSet::new(),
                        cancelled: Some(cancelled.clone()),
                        #[cfg(feature = "python-interop")]
                        python_bridge: None,
                        host_stderr: String::new(),
                        effect_report: Vec::new(),
                        omitted_effects: 0,
                        python_native_image_observation: "not_used".into(),
                        python_native_images: Vec::new(),
                    };
                    let result = worker.invoke_inner(
                        function,
                        vec![values[index].clone()],
                        depth,
                        budget.clone(),
                    );
                    match &result {
                        // The result may share parts with values this worker still holds.
                        Ok(value) => value.share_across_threads(),
                        Err(_) => cancelled.store(true, Ordering::Relaxed),
                    }
                    let _ = sender.send((index, result, worker.output, thread_id));
                }
            }));
        }
        drop(sender);
        let mut completed: Vec<Option<(Result<Value, RuntimeError>, String)>> =
            (0..values.len()).map(|_| None).collect();
        for (index, result, output, thread_id) in receiver {
            self.worker_threads.insert(thread_id);
            completed[index] = Some((result, output));
        }
        let mut worker_panicked = false;
        for handle in handles {
            worker_panicked |= handle.join().is_err();
        }
        budget.allocation.end_sharing();
        if budget.cancellation.is_cancelled() {
            return Err(RuntimeError::new(
                "R_CANCELLED",
                "invocation was cancelled while parallel workers were running",
                span,
            ));
        }
        let mut results = Vec::with_capacity(values.len());
        let mut first_error = None;
        for (index, item) in completed.into_iter().enumerate() {
            let Some((result, output)) = item else {
                continue;
            };
            if self.output.len().saturating_add(output.len()) > self.policy.output_limit_bytes {
                return Err(RuntimeError::new(
                    "R_OUTPUT_LIMIT",
                    "parallel worker output exceeds the configured limit",
                    span,
                ));
            }
            self.output.push_str(&output);
            match result {
                Ok(value) => results.push(value),
                Err(error) => {
                    if error.code != "R_CANCELLED" && first_error.is_none() {
                        first_error = Some((index, error));
                    }
                }
            }
        }
        if let Some((index, error)) = first_error {
            let mut trace = error.trace;
            trace.push(span);
            return Ok(Err(TaskErrorValue {
                index: BigInt::from_usize(index),
                code: error.code,
                message: error.message,
                trace,
            }));
        }
        if worker_panicked {
            return Err(RuntimeError::new(
                "R_WORKER_PANIC",
                "a parallel worker terminated unexpectedly",
                span,
            ));
        }
        if results.len() != values.len() {
            return Err(RuntimeError::new(
                "R_WORKER_CANCELLED",
                "parallel work ended before every input completed",
                span,
            ));
        }
        Ok(Ok(results))
    }
}

unsafe extern "C" fn native_capture_vm_continuation(
    arena: *mut NativeListArena,
    function_index: i64,
    resume_ip: i64,
    depth: i64,
    local_pairs: *const i64,
    local_count: i64,
    stack_pairs: *const i64,
    stack_count: i64,
    caller_ip: i64,
) -> i32 {
    let Some(function_index) = usize::try_from(function_index).ok() else {
        return 0;
    };
    let Some(resume_ip) = usize::try_from(resume_ip).ok() else {
        return 0;
    };
    let Some(depth) = usize::try_from(depth).ok() else {
        return 0;
    };
    let Some(local_count) = usize::try_from(local_count).ok() else {
        return 0;
    };
    let Some(stack_count) = usize::try_from(stack_count).ok() else {
        return 0;
    };
    if arena.is_null()
        || local_count > 16_384
        || stack_count > 16_384
        || (local_count > 0 && local_pairs.is_null())
        || (stack_count > 0 && stack_pairs.is_null())
    {
        return 0;
    }
    // SAFETY: generated code allocates triples (slot, tag, value) per live
    // local and invokes this helper while the slots are live.
    let local_triples = if local_count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(local_pairs, local_count.saturating_mul(3)) }
    };
    // SAFETY: same layout and lifetime guarantee as the local pair array above.
    let stack_pairs = if stack_count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(stack_pairs, stack_count.saturating_mul(2)) }
    };
    // SAFETY: the arena pointer belongs to this active NativeCode::invoke call.
    let arena = unsafe { &mut *arena };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(local_total) = arena
            .runtime_context
            .as_ref()
            .and_then(|context| context.artifact.functions.get(function_index))
            .map(|function| function.local_count)
        else {
            arena.native_vm_continuations.clear();
            return false;
        };
        if local_count > local_total {
            arena.native_vm_continuations.clear();
            return false;
        }
        let mut locals = vec![Value::Unit; local_total];
        let mut initialized = vec![false; local_total];
        for triple in local_triples.chunks_exact(3) {
            let Ok(slot) = usize::try_from(triple[0]) else {
                arena.native_vm_continuations.clear();
                return false;
            };
            if slot >= local_total || initialized[slot] {
                arena.native_vm_continuations.clear();
                return false;
            }
            let Some(value) = arena.decode_continuation_value(triple[2], triple[1]) else {
                arena.native_vm_continuations.clear();
                return false;
            };
            locals[slot] = value;
            initialized[slot] = true;
        }
        let decode_pairs = |pairs: &[i64]| -> Option<Vec<Value>> {
            pairs
                .chunks_exact(2)
                .map(|pair| arena.decode_continuation_value(pair[1], pair[0]))
                .collect()
        };
        let Some(stack) = decode_pairs(stack_pairs) else {
            arena.native_vm_continuations.clear();
            return false;
        };
        let caller_ip = if caller_ip < 0 {
            None
        } else {
            let Ok(caller_ip) = usize::try_from(caller_ip) else {
                arena.native_vm_continuations.clear();
                return false;
            };
            Some(caller_ip)
        };
        arena.append_native_continuation(
            NativeVmContinuationFrame {
                function_index,
                ip: resume_ip,
                depth,
                call_site: None,
                locals,
                stack,
            },
            caller_ip,
        )
    }));
    match result {
        Ok(true) => 1,
        Ok(false) | Err(_) => 0,
    }
}

unsafe extern "C" fn native_capture_vm_continuation_fixed(
    arena: *mut NativeListArena,
    function_index: i64,
    resume_ip: i64,
    depth: i64,
    stack_count: i64,
    first: i64,
    first_tag: i64,
    second: i64,
    second_tag: i64,
    caller_ip: i64,
) -> i32 {
    let Ok(stack_count) = usize::try_from(stack_count) else {
        return 0;
    };
    if stack_count > 2 {
        return 0;
    }
    let stack_pairs = [first_tag, first, second_tag, second];
    // SAFETY: the local array remains live through the synchronous capture call.
    unsafe {
        native_capture_vm_continuation(
            arena,
            function_index,
            resume_ip,
            depth,
            std::ptr::null(),
            0,
            stack_pairs.as_ptr(),
            stack_count as i64,
            caller_ip,
        )
    }
}

unsafe extern "C" fn native_capture_vm_continuation_int2(
    arena: *mut NativeListArena,
    function_index: i64,
    resume_ip: i64,
    depth: i64,
    first: i64,
    second: i64,
    caller_ip: i64,
) -> i32 {
    let stack_pairs = [1_i64, first, 1_i64, second];
    // SAFETY: the local array remains live through the synchronous capture call.
    unsafe {
        native_capture_vm_continuation(
            arena,
            function_index,
            resume_ip,
            depth,
            std::ptr::null(),
            0,
            stack_pairs.as_ptr(),
            2,
            caller_ip,
        )
    }
}

unsafe extern "C" fn native_reserve_fuel(budget: *const Budget, requested: u64) -> u64 {
    if budget.is_null() {
        return 0;
    }
    // The pointer is created from a live Arc<Budget> for the full JIT call.
    unsafe { (*budget).reserve(requested) }
}

unsafe extern "C" fn native_release_fuel(budget: *const Budget, count: u64) {
    if !budget.is_null() {
        // The pointer is created from a live Arc<Budget> for the full JIT call.
        unsafe { (*budget).refund(count) };
    }
}

unsafe extern "C" fn native_poll_cancellation(budget: *const Budget) -> i32 {
    if budget.is_null() {
        return 1;
    }
    // The pointer is created from a live Arc<Budget> for the full JIT call.
    i32::from(unsafe { (*budget).is_cancelled() })
}

const NATIVE_LIST_FLOAT_KIND: i64 = 1;
const NATIVE_LIST_INT_KIND: i64 = 2;
const NATIVE_LIST_BOOL_KIND: i64 = 3;
const NATIVE_LIST_HANDLE_KIND: i64 = 4;

unsafe extern "C" fn native_make_list(
    arena: *mut NativeListArena,
    values: *const i64,
    count: i64,
    item_kind: i64,
    typed_empty: i64,
) -> *mut std::ffi::c_void {
    if arena.is_null() || count < 0 || !(1..=4).contains(&item_kind) {
        return std::ptr::null_mut();
    }
    let arena = unsafe { &mut *arena };
    let Ok(count) = usize::try_from(count) else {
        return std::ptr::null_mut();
    };
    if count > arena.max_collection_items || (count > 0 && values.is_null()) {
        return std::ptr::null_mut();
    }
    let encoded = if count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(values, count) }
    };
    if item_kind == NATIVE_LIST_FLOAT_KIND {
        if encoded
            .iter()
            .any(|value| Float64::new(f64::from_bits(*value as u64)).is_none())
        {
            return std::ptr::null_mut();
        }
        let bytes = if typed_empty != 0 {
            size_of::<Vec<Float64>>()
        } else {
            size_of::<Vec<Value>>().saturating_add(count.saturating_mul(size_of::<Value>()))
        };
        if !arena.charge(bytes) {
            return std::ptr::null_mut();
        }
        if count <= NATIVE_INLINE_LIST_ITEMS {
            let mut descriptor = Box::new(NativeFloatList {
                data: std::ptr::null_mut(),
                len: count,
                storage_id: usize::MAX,
                owned: true,
                inline_storage: true,
                inline: [0; NATIVE_INLINE_LIST_ITEMS],
            });
            for (slot, value) in descriptor.inline.iter_mut().zip(encoded) {
                *slot = *value as u64;
            }
            descriptor.data = descriptor.inline.as_mut_ptr();
            arena.float_lists.push(descriptor);
            return (&mut **arena.float_lists.last_mut().unwrap() as *mut NativeFloatList).cast();
        }
        let decoded = encoded
            .iter()
            .map(|value| Float64::new(f64::from_bits(*value as u64)).unwrap())
            .collect::<Vec<_>>();
        let storage_id = arena.float_storage.len();
        arena.float_storage.push(decoded);
        arena.float_lists.push(Box::new(NativeFloatList {
            data: arena
                .float_storage
                .last_mut()
                .unwrap()
                .as_mut_ptr()
                .cast::<u64>(),
            len: count,
            storage_id,
            owned: true,
            inline_storage: false,
            inline: [0; NATIVE_INLINE_LIST_ITEMS],
        }));
        (&mut **arena.float_lists.last_mut().unwrap() as *mut NativeFloatList).cast()
    } else if item_kind == NATIVE_LIST_INT_KIND || item_kind == NATIVE_LIST_BOOL_KIND {
        if item_kind == NATIVE_LIST_BOOL_KIND
            && encoded.iter().any(|value| *value != 0 && *value != 1)
        {
            return std::ptr::null_mut();
        }
        let bytes =
            size_of::<Vec<Value>>().saturating_add(count.saturating_mul(size_of::<Value>()));
        if !arena.charge(bytes) {
            return std::ptr::null_mut();
        }
        if count <= NATIVE_INLINE_LIST_ITEMS {
            let mut descriptor = Box::new(NativeIntList {
                data: std::ptr::null_mut(),
                len: count,
                storage_id: usize::MAX,
                owned: true,
                inline_storage: true,
                inline: [0; NATIVE_INLINE_LIST_ITEMS],
            });
            descriptor.inline[..count].copy_from_slice(encoded);
            descriptor.data = descriptor.inline.as_mut_ptr();
            arena.int_lists.push(descriptor);
            return (&mut **arena.int_lists.last_mut().unwrap() as *mut NativeIntList).cast();
        }
        let storage_id = arena.int_storage.len();
        arena.int_storage.push(encoded.to_vec());
        arena.int_lists.push(Box::new(NativeIntList {
            data: arena.int_storage.last_mut().unwrap().as_mut_ptr(),
            len: count,
            storage_id,
            owned: true,
            inline_storage: false,
            inline: [0; NATIVE_INLINE_LIST_ITEMS],
        }));
        (&mut **arena.int_lists.last_mut().unwrap() as *mut NativeIntList).cast()
    } else if item_kind == NATIVE_LIST_HANDLE_KIND {
        let bytes = if typed_empty != 0 {
            size_of::<Vec<Value>>()
        } else {
            size_of::<Vec<Value>>().saturating_add(count.saturating_mul(size_of::<Value>()))
        };
        if !arena.charge(bytes) {
            return std::ptr::null_mut();
        }
        if count <= NATIVE_INLINE_LIST_ITEMS {
            let mut descriptor = Box::new(NativeHandleList {
                data: std::ptr::null_mut(),
                len: count,
                storage_id: usize::MAX,
                owned: true,
                inline_storage: true,
                inline: [0; NATIVE_INLINE_LIST_ITEMS],
            });
            descriptor.inline[..count].copy_from_slice(encoded);
            descriptor.data = descriptor.inline.as_mut_ptr();
            arena.handle_lists.push(descriptor);
            (&mut **arena.handle_lists.last_mut().unwrap() as *mut NativeHandleList).cast()
        } else {
            let storage_id = arena.handle_storage.len();
            arena.handle_storage.push(encoded.to_vec());
            arena.handle_lists.push(Box::new(NativeHandleList {
                data: arena.handle_storage.last_mut().unwrap().as_mut_ptr(),
                len: count,
                storage_id,
                owned: true,
                inline_storage: false,
                inline: [0; NATIVE_INLINE_LIST_ITEMS],
            }));
            (&mut **arena.handle_lists.last_mut().unwrap() as *mut NativeHandleList).cast()
        }
    } else {
        std::ptr::null_mut()
    }
}

#[inline(always)]
unsafe fn native_list_set_impl<const ITEM_KIND: i64, const ALWAYS_COPY: bool>(
    arena: *mut NativeListArena,
    list: *mut std::ffi::c_void,
    index: i64,
    value: i64,
) -> *mut std::ffi::c_void {
    if arena.is_null() || list.is_null() || index < 0 {
        return std::ptr::null_mut();
    }
    let arena = unsafe { &mut *arena };
    let Ok(index) = usize::try_from(index) else {
        return std::ptr::null_mut();
    };
    if ITEM_KIND == NATIVE_LIST_FLOAT_KIND {
        let descriptor = unsafe { &mut *(list.cast::<NativeFloatList>()) };
        if index >= descriptor.len {
            return std::ptr::null_mut();
        }
        arena.record_list_set_operation();
        let Some(value) = Float64::new(f64::from_bits(value as u64)) else {
            return std::ptr::null_mut();
        };
        if !ALWAYS_COPY && descriptor.owned {
            if descriptor.inline_storage {
                descriptor.inline[index] = value.get().to_bits();
            } else {
                let Some(storage) = arena.float_storage.get_mut(descriptor.storage_id) else {
                    return std::ptr::null_mut();
                };
                storage[index] = value;
            }
            arena.record_list_set_in_place_update();
            return list;
        }
        let list_len = descriptor.len;
        let bytes =
            size_of::<Vec<Float64>>().saturating_add(list_len.saturating_mul(size_of::<Float64>()));
        if !arena.charge(bytes) {
            return std::ptr::null_mut();
        }
        if list_len <= NATIVE_INLINE_LIST_ITEMS {
            let source = unsafe { std::slice::from_raw_parts(descriptor.data, list_len) };
            let mut copied = Box::new(NativeFloatList {
                data: std::ptr::null_mut(),
                len: list_len,
                storage_id: usize::MAX,
                owned: true,
                inline_storage: true,
                inline: [0; NATIVE_INLINE_LIST_ITEMS],
            });
            copied.inline[..list_len].copy_from_slice(source);
            copied.inline[index] = value.get().to_bits();
            copied.data = copied.inline.as_mut_ptr();
            arena.record_list_set_copy(list_len, bytes);
            arena.float_lists.push(copied);
            return (&mut **arena.float_lists.last_mut().unwrap() as *mut NativeFloatList).cast();
        }
        let Some(storage) = arena.float_storage.get(descriptor.storage_id) else {
            return std::ptr::null_mut();
        };
        let mut copied = storage.clone();
        copied[index] = value;
        arena.record_list_set_copy(list_len, bytes);
        let storage_id = arena.float_storage.len();
        arena.float_storage.push(copied);
        arena.float_lists.push(Box::new(NativeFloatList {
            data: arena
                .float_storage
                .last_mut()
                .unwrap()
                .as_mut_ptr()
                .cast::<u64>(),
            len: list_len,
            storage_id,
            owned: true,
            inline_storage: false,
            inline: [0; NATIVE_INLINE_LIST_ITEMS],
        }));
        (&mut **arena.float_lists.last_mut().unwrap() as *mut NativeFloatList).cast()
    } else {
        let descriptor = unsafe { &mut *(list.cast::<NativeIntList>()) };
        if index >= descriptor.len {
            return std::ptr::null_mut();
        }
        arena.record_list_set_operation();
        if ITEM_KIND == NATIVE_LIST_BOOL_KIND && value != 0 && value != 1 {
            return std::ptr::null_mut();
        }
        if !ALWAYS_COPY && descriptor.owned {
            if descriptor.inline_storage {
                descriptor.inline[index] = value;
            } else {
                let Some(storage) = arena.int_storage.get_mut(descriptor.storage_id) else {
                    return std::ptr::null_mut();
                };
                storage[index] = value;
            }
            arena.record_list_set_in_place_update();
            return list;
        }
        let list_len = descriptor.len;
        let bytes =
            size_of::<Vec<Value>>().saturating_add(list_len.saturating_mul(size_of::<Value>()));
        if !arena.charge(bytes) {
            return std::ptr::null_mut();
        }
        if list_len <= NATIVE_INLINE_LIST_ITEMS {
            let source = unsafe { std::slice::from_raw_parts(descriptor.data, list_len) };
            arena.record_list_set_copy(list_len, bytes);
            return arena.allocate_inline_int(source, index, value);
        }
        let Some(storage) = arena.int_storage.get(descriptor.storage_id) else {
            return std::ptr::null_mut();
        };
        let mut copied = storage.clone();
        copied[index] = value;
        arena.record_list_set_copy(list_len, bytes);
        let storage_id = arena.int_storage.len();
        arena.int_storage.push(copied);
        arena.int_lists.push(Box::new(NativeIntList {
            data: arena.int_storage.last_mut().unwrap().as_mut_ptr(),
            len: list_len,
            storage_id,
            owned: true,
            inline_storage: false,
            inline: [0; NATIVE_INLINE_LIST_ITEMS],
        }));
        (&mut **arena.int_lists.last_mut().unwrap() as *mut NativeIntList).cast()
    }
}

#[cfg(test)]
unsafe extern "C" fn native_list_set(
    arena: *mut NativeListArena,
    list: *mut std::ffi::c_void,
    index: i64,
    value: i64,
    item_kind: i64,
    always_copy: i64,
) -> *mut std::ffi::c_void {
    match (item_kind, always_copy == 0) {
        (NATIVE_LIST_FLOAT_KIND, true) => unsafe {
            native_list_set_impl::<NATIVE_LIST_FLOAT_KIND, false>(arena, list, index, value)
        },
        (NATIVE_LIST_INT_KIND, true) => unsafe {
            native_list_set_impl::<NATIVE_LIST_INT_KIND, false>(arena, list, index, value)
        },
        (NATIVE_LIST_BOOL_KIND, true) => unsafe {
            native_list_set_impl::<NATIVE_LIST_BOOL_KIND, false>(arena, list, index, value)
        },
        (NATIVE_LIST_FLOAT_KIND, false) => unsafe {
            native_list_set_impl::<NATIVE_LIST_FLOAT_KIND, true>(arena, list, index, value)
        },
        (NATIVE_LIST_INT_KIND, false) => unsafe {
            native_list_set_impl::<NATIVE_LIST_INT_KIND, true>(arena, list, index, value)
        },
        (NATIVE_LIST_BOOL_KIND, false) => unsafe {
            native_list_set_impl::<NATIVE_LIST_BOOL_KIND, true>(arena, list, index, value)
        },
        _ => std::ptr::null_mut(),
    }
}

macro_rules! native_list_set_entry {
    ($name:ident, $item_kind:ident, $always_copy:literal) => {
        unsafe extern "C" fn $name(
            arena: *mut NativeListArena,
            list: *mut std::ffi::c_void,
            index: i64,
            value: i64,
        ) -> *mut std::ffi::c_void {
            unsafe { native_list_set_impl::<$item_kind, $always_copy>(arena, list, index, value) }
        }
    };
}

native_list_set_entry!(native_list_set_float_copy, NATIVE_LIST_FLOAT_KIND, true);
native_list_set_entry!(native_list_set_int_copy, NATIVE_LIST_INT_KIND, true);
native_list_set_entry!(native_list_set_bool_copy, NATIVE_LIST_BOOL_KIND, true);
native_list_set_entry!(native_list_set_float_local, NATIVE_LIST_FLOAT_KIND, false);
native_list_set_entry!(native_list_set_int_local, NATIVE_LIST_INT_KIND, false);
native_list_set_entry!(native_list_set_bool_local, NATIVE_LIST_BOOL_KIND, false);

unsafe extern "C" fn native_charge_record(arena: *mut NativeListArena, bytes: i64) -> i32 {
    if arena.is_null() || bytes < 0 {
        return 0;
    }
    let Ok(bytes) = usize::try_from(bytes) else {
        return 0;
    };
    let arena = unsafe { &mut *arena };
    let ok = arena.charge(bytes);
    if ok {
        arena.record_record_charge(bytes);
    }
    i32::from(ok)
}

extern "C" fn native_math(code: i64, a: f64, b: f64) -> f64 {
    u8::try_from(code)
        .ok()
        .and_then(guion_model::MathFunction::from_code)
        .map_or(f64::NAN, |function| {
            guion_vm_core::math_or_nan(function, a, b)
        })
}

unsafe extern "C" fn native_make_tagged_value(
    arena: *mut NativeListArena,
    kind: i64,
    payload_tag: i64,
    payload: i64,
) -> i64 {
    if arena.is_null() {
        return 0;
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        (&mut *arena).store_native_tagged_value(kind, payload_tag, payload)
    })) {
        Ok(Some(handle)) => handle,
        Ok(None) | Err(_) => 0,
    }
}

unsafe extern "C" fn native_charge_value_clone(arena: *mut NativeListArena, handle: i64) -> i32 {
    if arena.is_null() {
        return 0;
    }
    let arena = unsafe { &mut *arena };
    if let Some(value) = arena.constant_value(handle) {
        return i32::from(arena.charge(cloned_value_allocation(value)));
    }
    if handle < 0 {
        let Some(bytes) = arena.tagged_clone_charge_bytes(handle) else {
            return 0;
        };
        return i32::from(arena.charge(bytes));
    }
    let Ok(index) = usize::try_from(handle.checked_sub(1).unwrap_or(-1)) else {
        return 0;
    };
    let Some(bytes) = arena.values.get(index).map(cloned_value_allocation) else {
        return 0;
    };
    i32::from(arena.charge(bytes))
}

// This is the fixed callback ABI emitted by Cranelift; its arguments cannot be bundled.
#[allow(clippy::too_many_arguments)]
unsafe fn native_direct_operation_impl(
    operation: i64,
    argument_count: usize,
    arena: *mut NativeListArena,
    function_index: i64,
    instruction_index: i64,
    depth: i64,
    first: i64,
    first_tag: i64,
    second: i64,
    second_tag: i64,
    third: i64,
    third_tag: i64,
    fourth: i64,
    fourth_tag: i64,
    fifth: i64,
    fifth_tag: i64,
    result_tag: i64,
    output: *mut i64,
) -> i32 {
    if arena.is_null() || output.is_null() || argument_count > 5 {
        return 0;
    }
    let values = [first, second, third, fourth, fifth];
    let tags = [first_tag, second_tag, third_tag, fourth_tag, fifth_tag];
    // SAFETY: these pointers are supplied by the active JIT invocation.
    let arena = unsafe { &mut *arena };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        arena.execute_native_operation(NativeOperationRequest {
            operation,
            function_index,
            instruction_index,
            depth,
            arguments: &values[..argument_count],
            argument_tags: &tags[..argument_count],
            result_tag,
        })
    })) {
        Ok(Some(value)) => {
            // SAFETY: the generated caller provides one writable i64 result slot.
            unsafe { output.write(value) };
            1
        }
        Ok(None) => 0,
        Err(_) => {
            arena.record_bridge_panic(function_index, instruction_index);
            0
        }
    }
}

macro_rules! native_direct_operation_entry {
    ($name:ident, $operation:expr, $argument_count:expr) => {
        unsafe extern "C" fn $name(
            arena: *mut NativeListArena,
            function_index: i64,
            instruction_index: i64,
            depth: i64,
            first: i64,
            first_tag: i64,
            second: i64,
            second_tag: i64,
            third: i64,
            third_tag: i64,
            fourth: i64,
            fourth_tag: i64,
            fifth: i64,
            fifth_tag: i64,
            result_tag: i64,
            output: *mut i64,
        ) -> i32 {
            unsafe {
                native_direct_operation_impl(
                    $operation,
                    $argument_count,
                    arena,
                    function_index,
                    instruction_index,
                    depth,
                    first,
                    first_tag,
                    second,
                    second_tag,
                    third,
                    third_tag,
                    fourth,
                    fourth_tag,
                    fifth,
                    fifth_tag,
                    result_tag,
                    output,
                )
            }
        }
    };
}

native_direct_operation_entry!(
    native_direct_json_parse,
    native_backend::NATIVE_BRIDGE_JSON_PARSE,
    1
);
native_direct_operation_entry!(
    native_direct_json_try_text,
    native_backend::NATIVE_BRIDGE_JSON_TRY_TEXT,
    1
);
native_direct_operation_entry!(
    native_direct_json_try_get,
    native_backend::NATIVE_BRIDGE_JSON_TRY_GET,
    2
);
native_direct_operation_entry!(
    native_direct_json_try_int,
    native_backend::NATIVE_BRIDGE_JSON_TRY_INT,
    1
);
native_direct_operation_entry!(
    native_direct_file_next_line,
    native_backend::NATIVE_BRIDGE_FILE_NEXT_LINE,
    1
);
native_direct_operation_entry!(
    native_direct_get_field,
    native_backend::NATIVE_BRIDGE_GET_FIELD,
    1
);
native_direct_operation_entry!(native_direct_args, native_backend::NATIVE_BRIDGE_ARGS, 0);
native_direct_operation_entry!(
    native_direct_text_equal,
    native_backend::NATIVE_BRIDGE_TEXT_EQUAL,
    2
);
native_direct_operation_entry!(
    native_direct_print_err,
    native_backend::NATIVE_BRIDGE_PRINT_ERR,
    1
);
native_direct_operation_entry!(
    native_direct_make_typed_map,
    native_backend::NATIVE_BRIDGE_MAKE_TYPED_MAP,
    0
);
native_direct_operation_entry!(
    native_direct_json_array,
    native_backend::NATIVE_BRIDGE_JSON_ARRAY,
    0
);
native_direct_operation_entry!(
    native_direct_map_keys,
    native_backend::NATIVE_BRIDGE_MAP_KEYS,
    1
);
native_direct_operation_entry!(
    native_direct_json_index,
    native_backend::NATIVE_BRIDGE_JSON_INDEX,
    2
);
native_direct_operation_entry!(
    native_direct_text_chars,
    native_backend::NATIVE_BRIDGE_TEXT_CHARS,
    1
);
native_direct_operation_entry!(
    native_direct_json_get,
    native_backend::NATIVE_BRIDGE_JSON_GET,
    2
);
native_direct_operation_entry!(
    native_direct_json_stringify,
    native_backend::NATIVE_BRIDGE_JSON_STRINGIFY,
    1
);
native_direct_operation_entry!(
    native_direct_text_concat,
    native_backend::NATIVE_BRIDGE_TEXT_CONCAT,
    2
);
native_direct_operation_entry!(
    native_direct_json_object,
    native_backend::NATIVE_BRIDGE_JSON_OBJECT,
    0
);
native_direct_operation_entry!(
    native_direct_map_index,
    native_backend::NATIVE_BRIDGE_MAP_INDEX,
    2
);
native_direct_operation_entry!(
    native_direct_map_set,
    native_backend::NATIVE_BRIDGE_MAP_SET,
    5
);
native_direct_operation_entry!(
    native_direct_json_int,
    native_backend::NATIVE_BRIDGE_JSON_INT,
    1
);
native_direct_operation_entry!(
    native_direct_map_has,
    native_backend::NATIVE_BRIDGE_MAP_HAS,
    2
);
native_direct_operation_entry!(
    native_direct_json_text,
    native_backend::NATIVE_BRIDGE_JSON_TEXT,
    1
);
native_direct_operation_entry!(
    native_direct_file_open_lines,
    native_backend::NATIVE_BRIDGE_FILE_OPEN_LINES,
    1
);
native_direct_operation_entry!(
    native_direct_file_close,
    native_backend::NATIVE_BRIDGE_FILE_CLOSE,
    1
);
native_direct_operation_entry!(
    native_direct_text_collapse_whitespace,
    native_backend::NATIVE_BRIDGE_TEXT_COLLAPSE_WHITESPACE,
    1
);
native_direct_operation_entry!(
    native_direct_time_parse_timestamp,
    native_backend::NATIVE_BRIDGE_TIME_PARSE_TIMESTAMP,
    1
);
native_direct_operation_entry!(
    native_direct_time_format_utc,
    native_backend::NATIVE_BRIDGE_TIME_FORMAT_UTC,
    1
);
native_direct_operation_entry!(
    native_direct_text_slice,
    native_backend::NATIVE_BRIDGE_TEXT_SLICE,
    3
);
native_direct_operation_entry!(
    native_direct_json_set_local,
    native_backend::NATIVE_BRIDGE_JSON_SET_LOCAL,
    4
);
native_direct_operation_entry!(
    native_direct_json_push_local,
    native_backend::NATIVE_BRIDGE_JSON_PUSH_LOCAL,
    3
);
native_direct_operation_entry!(
    native_direct_text_find,
    native_backend::NATIVE_BRIDGE_TEXT_FIND,
    3
);
native_direct_operation_entry!(
    native_direct_text_split,
    native_backend::NATIVE_BRIDGE_TEXT_SPLIT,
    2
);
native_direct_operation_entry!(
    native_direct_text_replace,
    native_backend::NATIVE_BRIDGE_TEXT_REPLACE,
    3
);
native_direct_operation_entry!(
    native_direct_text_trim,
    native_backend::NATIVE_BRIDGE_TEXT_TRIM,
    1
);
native_direct_operation_entry!(
    native_direct_text_join,
    native_backend::NATIVE_BRIDGE_TEXT_JOIN,
    2
);
native_direct_operation_entry!(
    native_direct_text_length,
    native_backend::NATIVE_BRIDGE_TEXT_LENGTH,
    1
);
unsafe extern "C" fn native_direct_make_record(
    arena: *mut NativeListArena,
    function_index: i64,
    instruction_index: i64,
    depth: i64,
    values: *const i64,
    tags: *const i64,
    argument_count: i64,
    result_tag: i64,
    output: *mut i64,
) -> i32 {
    let Ok(argument_count) = usize::try_from(argument_count) else {
        return 0;
    };
    if arena.is_null()
        || output.is_null()
        || (argument_count > 0 && (values.is_null() || tags.is_null()))
        || argument_count > 16_384
    {
        return 0;
    }
    let values = if argument_count == 0 {
        &[]
    } else {
        // SAFETY: generated code allocates both arrays for this exact count.
        unsafe { std::slice::from_raw_parts(values, argument_count) }
    };
    let tags = if argument_count == 0 {
        &[]
    } else {
        // SAFETY: generated code allocates both arrays for this exact count.
        unsafe { std::slice::from_raw_parts(tags, argument_count) }
    };
    // SAFETY: the arena belongs to the synchronous native invocation.
    let arena = unsafe { &mut *arena };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        arena.execute_native_operation(NativeOperationRequest {
            operation: native_backend::NATIVE_BRIDGE_MAKE_RECORD,
            function_index,
            instruction_index,
            depth,
            arguments: values,
            argument_tags: tags,
            result_tag,
        })
    })) {
        Ok(Some(value)) => {
            // SAFETY: the generated caller provides one writable i64 result slot.
            unsafe { output.write(value) };
            1
        }
        Ok(None) => 0,
        Err(_) => {
            arena.record_bridge_panic(function_index, instruction_index);
            0
        }
    }
}

/// Loop-backedge safepoint. `triples` points at `count` `(slot, tag, handle)` words.
/// Returns non-zero only when the arena failed after being emptied.
unsafe extern "C" fn native_loop_safepoint(
    arena: *mut NativeListArena,
    triples: *mut [i64; 3],
    count: i64,
) -> i32 {
    if arena.is_null() {
        return 0;
    }
    let arena = unsafe { &mut *arena };
    if arena.retained_entries() < arena.reclaim_floor {
        return 0;
    }
    let roots: &mut [[i64; 3]] = if triples.is_null() || count <= 0 {
        &mut []
    } else {
        unsafe { std::slice::from_raw_parts_mut(triples, count as usize) }
    };
    arena.reclaim_dead_handles(roots);
    i32::from(arena.host_error.is_some())
}

unsafe extern "C" fn native_count_guest_branch(arena: *mut NativeListArena) {
    if !arena.is_null() {
        unsafe { &*arena }.record_guest_conditional_branch();
    }
}

unsafe extern "C" fn native_print(arena: *mut NativeListArena, value: i64, kind: i64) -> i32 {
    if arena.is_null() {
        return 0;
    }
    i32::from(unsafe { &mut *arena }.print(value, kind))
}

fn http_effect_target(method: &str, url: &str) -> String {
    let host = reqwest::Url::parse(url)
        .ok()
        .and_then(|url| {
            let host = url.host_str()?;
            Some(match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_owned(),
            })
        })
        .unwrap_or_else(|| "invalid-url".into());
    format!("{method} {host}")
}

fn native_runtime_error(
    artifact: &Artifact,
    native_trace: &[(usize, usize)],
    code: impl Into<String>,
    message: impl Into<String>,
    fault_span: Span,
) -> RuntimeError {
    let mut error = RuntimeError::new(code, message, fault_span);
    for (function_index, instruction_index) in native_trace.iter().rev() {
        if let Some(span) = artifact
            .functions
            .get(*function_index)
            .and_then(|function| function.code.get(*instruction_index))
            .map(|instruction| instruction.span)
        {
            error.trace.push(span);
        }
    }
    error
}

fn input_value_allocation(value: &Value) -> usize {
    match value {
        Value::Record(name, fields) => size_of::<Value>()
            .saturating_add(size_of::<String>() + name.len())
            .saturating_add(size_of::<Vec<Value>>())
            .saturating_add(fields.len().saturating_mul(size_of::<Value>()))
            .saturating_add(
                fields
                    .iter()
                    .map(input_value_allocation)
                    .fold(0usize, usize::saturating_add),
            ),
        Value::List(items) => size_of::<Value>()
            .saturating_add(size_of::<Vec<Value>>())
            .saturating_add(items.len().saturating_mul(size_of::<Value>()))
            .saturating_add(
                items
                    .iter()
                    .map(input_value_allocation)
                    .fold(0usize, usize::saturating_add),
            ),
        Value::Float64List(items) => size_of::<Value>()
            .saturating_add(size_of::<Vec<Float64>>())
            .saturating_add(items.len().saturating_mul(size_of::<Float64>())),
        Value::Array(array) => {
            size_of::<Value>().saturating_add(guion_vm_core::array_allocation(array))
        }
        Value::Map(items) => size_of::<Value>().saturating_add(
            items
                .iter()
                .map(|(key, value)| {
                    size_of::<String>()
                        .saturating_add(key.len())
                        .saturating_add(size_of::<Value>() * 3)
                        .saturating_add(input_value_allocation(value))
                })
                .fold(0usize, usize::saturating_add),
        ),
        Value::Json(_) => 0,
        Value::OptionSome(value) | Value::ResultOk(value) | Value::ResultErr(value) => {
            size_of::<Value>().saturating_add(input_value_allocation(value))
        }
        Value::Int(value) => size_of::<Value>() + size_of::<BigInt>() + value.digit_count(),
        Value::Text(value) => size_of::<Value>() + size_of::<String>() + value.len(),
        Value::Bytes(value) => size_of::<Value>() + size_of::<Vec<u8>>() + value.len(),
        Value::TaskError(error) => {
            size_of::<Value>()
                + size_of::<TaskErrorValue>()
                + error.code.len()
                + error.message.len()
                + error.trace.len().saturating_mul(size_of::<Span>())
        }
        Value::JsonError(error) => {
            size_of::<Value>()
                + size_of::<JsonErrorValue>()
                + error.code.len()
                + error.message.len()
        }
        Value::FileError(error) => {
            size_of::<Value>()
                + size_of::<FileErrorValue>()
                + error.code.len()
                + error.message.len()
                + error.trace.len().saturating_mul(size_of::<Span>())
        }
        Value::HttpError(error) => {
            size_of::<Value>()
                + size_of::<HttpErrorValue>()
                + error.code.len()
                + error.message.len()
                + error.trace.len().saturating_mul(size_of::<Span>())
        }
        Value::NativeResource(resource) => size_of::<Value>() + resource.kind().len(),
        Value::Float64(_) | Value::Bool(_) | Value::OptionNone | Value::Unit => size_of::<Value>(),
    }
}

fn file_error_result(error: RuntimeError) -> Value {
    Value::ResultErr(Box::new(Value::FileError(Box::new(FileErrorValue {
        code: error.code,
        message: error.message,
        trace: error.trace,
    }))))
}

fn runtime_reader_handle(
    value: &Value,
    span: Span,
) -> Result<stdlib_data::FileReaderHandle, RuntimeError> {
    let Value::NativeResource(resource) = value else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "file reader operation requires FileReader",
            span,
        ));
    };
    let (kind, owner, id) = resource.parts();
    if kind != "FileReader" {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "resource is not a FileReader",
            span,
        ));
    }
    Ok(stdlib_data::FileReaderHandle::from_parts(owner, id))
}

fn runtime_service_resource(
    value: &Value,
    expected_kind: &str,
    span: Span,
) -> Result<(u64, u64), RuntimeError> {
    let Value::NativeResource(resource) = value else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            format!("operation requires {expected_kind}"),
            span,
        ));
    };
    let (kind, owner, id) = resource.parts();
    if kind != expected_kind {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            format!("operation requires {expected_kind}"),
            span,
        ));
    }
    Ok((owner, id))
}

fn runtime_text_path(value: &Value, span: Span) -> Result<Vec<String>, RuntimeError> {
    let Value::List(items) = value else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "TOML path must be List[Text]",
            span,
        ));
    };
    items
        .iter()
        .map(|item| match item {
            Value::Text(text) => Ok(text.to_string()),
            _ => Err(RuntimeError::new(
                "R_TYPE_INVALID",
                "TOML path must contain only Text",
                span,
            )),
        })
        .collect()
}

fn runtime_sql_values(
    value: &Value,
    span: Span,
) -> Result<Vec<stdlib_services::SqlValue>, RuntimeError> {
    let Value::List(items) = value else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "SQLite parameters must be List[SqlValue]",
            span,
        ));
    };
    items
        .iter()
        .map(|item| {
            let Value::Record(name, fields) = item else {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "SQLite parameter must be SqlValue",
                    span,
                ));
            };
            if name != "SqlValue" || fields.len() != 5 {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "invalid SqlValue record",
                    span,
                ));
            }
            let (
                Value::Text(kind),
                Value::Int(integer),
                Value::Float64(real),
                Value::Text(text),
                Value::Bytes(blob),
            ) = (&fields[0], &fields[1], &fields[2], &fields[3], &fields[4])
            else {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "invalid SqlValue payload",
                    span,
                ));
            };
            let integer_i64 = integer.to_i64();
            let zero_integer = integer == &BigInt::zero();
            let zero_real = real.get() == 0.0;
            let empty_text = text.is_empty();
            let empty_blob = blob.is_empty();
            let decoded = match kind.as_ref() {
                "null" if zero_integer && zero_real && empty_text && empty_blob => {
                    stdlib_services::SqlValue::Null
                }
                "integer" if integer_i64.is_some() && zero_real && empty_text && empty_blob => {
                    stdlib_services::SqlValue::Integer(integer_i64.unwrap())
                }
                "real" if zero_integer && real.get().is_finite() && empty_text && empty_blob => {
                    stdlib_services::SqlValue::Real(real.get())
                }
                "text" if zero_integer && zero_real && empty_blob => {
                    stdlib_services::SqlValue::Text(text.to_string())
                }
                "blob" if zero_integer && zero_real && empty_text => {
                    stdlib_services::SqlValue::Blob(blob.as_ref().clone())
                }
                _ => return Err(RuntimeError::new(
                    "R_SQLITE_VALUE",
                    "SqlValue kind and payload fields are inconsistent or outside SQLite's range",
                    span,
                )),
            };
            Ok(decoded)
        })
        .collect()
}

fn service_error_value(error: stdlib_services::ServiceError) -> Value {
    Value::Record(
        "StdError".into(),
        Shared::new(vec![
            Value::Text(error.code.into()),
            Value::Text(error.message.into()),
            Value::Text(error.path.into()),
            Value::Int(BigInt::from_usize(error.line)),
            Value::Int(BigInt::from_usize(error.column)),
        ]),
    )
}

fn temporal_resolution_result_value(
    result: Result<stdlib_services::TemporalResolution, stdlib_services::ServiceError>,
) -> Value {
    match result {
        Ok(resolution) => Value::ResultOk(Box::new(temporal_resolution_value(resolution))),
        Err(error) => Value::ResultErr(Box::new(Value::Record(
            "TimeError".into(),
            Shared::new(vec![
                Value::Text(error.code.into()),
                Value::Text(error.message.into()),
            ]),
        ))),
    }
}

fn service_result_value(result: Result<Value, stdlib_services::ServiceError>) -> Value {
    match result {
        Ok(value) => Value::ResultOk(Box::new(value)),
        Err(error) => Value::ResultErr(Box::new(service_error_value(error))),
    }
}

fn service_sql_value(value: stdlib_services::SqlValue) -> Value {
    let (kind, integer, real, text, blob) = match value {
        stdlib_services::SqlValue::Null => ("null", 0, 0.0, String::new(), Vec::new()),
        stdlib_services::SqlValue::Integer(value) => {
            ("integer", value, 0.0, String::new(), Vec::new())
        }
        stdlib_services::SqlValue::Real(value) => ("real", 0, value, String::new(), Vec::new()),
        stdlib_services::SqlValue::Text(value) => ("text", 0, 0.0, value, Vec::new()),
        stdlib_services::SqlValue::Blob(value) => ("blob", 0, 0.0, String::new(), value),
    };
    Value::Record(
        "SqlValue".into(),
        Shared::new(vec![
            Value::Text(kind.into()),
            Value::Int(BigInt::from_i64(integer)),
            Value::Float64(Float64::new(real).expect("SQLite service returns finite reals")),
            Value::Text(text.into()),
            Value::Bytes(Shared::new(blob)),
        ]),
    )
}

fn service_calendar_event(event: stdlib_services::CalendarEvent) -> Value {
    let resolution = |resolution: Option<stdlib_services::TemporalResolution>| {
        resolution
            .map(|resolution| Value::OptionSome(Box::new(temporal_resolution_value(resolution))))
            .unwrap_or(Value::OptionNone)
    };
    let date = |date: Option<(i32, u8, u8)>| {
        date.map(|(year, month, day)| {
            Value::OptionSome(Box::new(Value::Record(
                "Date".into(),
                Shared::new(vec![
                    Value::Int(BigInt::from_i64(i64::from(year))),
                    Value::Int(BigInt::from_i64(i64::from(month))),
                    Value::Int(BigInt::from_i64(i64::from(day))),
                ]),
            )))
        })
        .unwrap_or(Value::OptionNone)
    };
    let recurrence = event
        .recurrence_rule
        .map(|rule| Value::OptionSome(Box::new(Value::Text(rule.into()))))
        .unwrap_or(Value::OptionNone);
    Value::Record(
        "CalendarEvent".into(),
        Shared::new(vec![
            Value::Text(event.uid.into()),
            Value::Text(event.summary.into()),
            resolution(event.start_resolution),
            resolution(event.end_resolution),
            date(event.start_date),
            date(event.end_date),
            Value::Text(event.status.into()),
            Value::Bool(event.all_day),
            recurrence,
        ]),
    )
}

fn temporal_resolution_value(resolution: stdlib_services::TemporalResolution) -> Value {
    let local = resolution.local;
    let instants = resolution
        .unix_microseconds
        .into_iter()
        .map(|microseconds| {
            Value::Record(
                "Instant".into(),
                Shared::new(vec![Value::Int(BigInt::from_i64(microseconds))]),
            )
        })
        .collect();
    Value::Record(
        "TemporalResolution".into(),
        Shared::new(vec![
            Value::Text(resolution.kind.into()),
            Value::Record(
                "LocalDateTime".into(),
                Shared::new(vec![
                    Value::Int(BigInt::from_i64(local.year)),
                    Value::Int(BigInt::from_i64(local.month)),
                    Value::Int(BigInt::from_i64(local.day)),
                    Value::Int(BigInt::from_i64(local.hour)),
                    Value::Int(BigInt::from_i64(local.minute)),
                    Value::Int(BigInt::from_i64(local.second)),
                    Value::Int(BigInt::from_i64(local.microsecond)),
                ]),
            ),
            Value::Text(resolution.timezone.into()),
            Value::from_list_items(instants),
        ]),
    )
}

fn reader_runtime_error(
    kind: stdlib_data::ReaderErrorKind,
    message: String,
    line: usize,
    column: usize,
    span: Span,
) -> RuntimeError {
    let code = match kind {
        stdlib_data::ReaderErrorKind::Closed => "R_RESOURCE_CLOSED",
        stdlib_data::ReaderErrorKind::Stale => "R_RESOURCE_STALE",
        stdlib_data::ReaderErrorKind::ForeignOwner => "R_RESOURCE_OWNER",
        stdlib_data::ReaderErrorKind::Mode => "R_RESOURCE_MODE",
        stdlib_data::ReaderErrorKind::InvalidUtf8 => "R_FILE_UTF8",
        stdlib_data::ReaderErrorKind::Limit => "R_FILE_LIMIT",
        stdlib_data::ReaderErrorKind::Io => "R_FILE_READ",
    };
    let mut error = RuntimeError::new(code, message, span);
    if line > 0 {
        error.trace.push(Span {
            line,
            column,
            ..span
        });
    }
    error
}

fn http_error(code: &str, message: impl Into<String>) -> HttpErrorValue {
    HttpErrorValue {
        code: code.into(),
        message: message.into(),
        trace: Vec::new(),
    }
}

#[derive(Debug)]
struct HostHttpClient {
    client: reqwest::Client,
    executor: tokio::runtime::Runtime,
}

impl HostHttpClient {
    fn new(max_parallelism: usize) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| error.to_string())?;
        let executor = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(max_parallelism.clamp(1, 8))
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { client, executor })
    }
}

#[derive(Debug)]
struct HttpBytesResponse {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl HttpBytesResponse {
    fn into_text_json(self, max_text_bytes: usize) -> Result<serde_json::Value, HttpErrorValue> {
        if self.body.len() > max_text_bytes {
            return Err(http_error(
                "R_HTTP_LIMIT",
                "HTTP response exceeds the configured text byte limit",
            ));
        }
        let body = String::from_utf8(self.body)
            .map_err(|_| http_error("R_HTTP_ENCODING", "HTTP response body is not valid UTF-8"))?;
        Ok(serde_json::json!({
            "status": self.status,
            "headers": self.headers,
            "body": body,
        }))
    }
}

fn validate_http_url(url: &reqwest::Url, policy: &HostPolicy) -> Result<String, HttpErrorValue> {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(http_error(
            "R_HTTP_URL",
            "only HTTP and HTTPS URLs without embedded credentials are allowed",
        ));
    }
    let host = url
        .host_str()
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
        .ok_or_else(|| http_error("R_HTTP_URL", "URL has no host"))?;
    if policy.allowed_http_hosts.is_empty() || !policy.allowed_http_hosts.contains(&host) {
        return Err(http_error(
            "R_NETWORK_DENIED",
            "network access to this host is not granted",
        ));
    }
    Ok(host)
}

fn request_headers(
    headers: &BTreeMap<String, String>,
    policy: &HostPolicy,
) -> Result<reqwest::header::HeaderMap, HttpErrorValue> {
    let mut result = reqwest::header::HeaderMap::new();
    let mut header_bytes = 0usize;
    for (name, value) in headers {
        header_bytes = header_bytes
            .saturating_add(name.len())
            .saturating_add(value.len());
        if header_bytes > policy.max_http_request_bytes {
            return Err(http_error(
                "R_HTTP_LIMIT",
                "HTTP request headers exceed the configured byte limit",
            ));
        }
        let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| http_error("R_HTTP_HEADER", "request header name is invalid"))?;
        if matches!(
            name.as_str(),
            "host" | "content-length" | "connection" | "transfer-encoding"
        ) {
            return Err(http_error(
                "R_HTTP_HEADER",
                "request header is managed by the HTTP client",
            ));
        }
        let value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| http_error("R_HTTP_HEADER", "request header value is invalid"))?;
        result.insert(name, value);
    }
    Ok(result)
}

async fn execute_http_bytes(
    client: &reqwest::Client,
    policy: &HostPolicy,
    method: &str,
    input: &str,
    headers: &BTreeMap<String, String>,
    body: &[u8],
    deadline: Instant,
) -> Result<HttpBytesResponse, HttpErrorValue> {
    if body.len() > policy.max_http_request_bytes {
        return Err(http_error(
            "R_HTTP_LIMIT",
            "HTTP request body exceeds the configured byte limit",
        ));
    }
    let mut method = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|_| http_error("R_HTTP_METHOD", "HTTP method is invalid"))?;
    if !matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
    ) {
        return Err(http_error("R_HTTP_METHOD", "HTTP method is not supported"));
    }
    let mut url =
        reqwest::Url::parse(input).map_err(|_| http_error("R_HTTP_URL", "URL is invalid"))?;
    let initial_host = validate_http_url(&url, policy)?;
    let initial_origin = url.origin().ascii_serialization();
    let mut headers = request_headers(headers, policy)?;
    let mut body = body.to_vec();

    for redirect_count in 0..=policy.max_http_redirects {
        let host = validate_http_url(&url, policy)?;
        if host != initial_host || url.origin().ascii_serialization() != initial_origin {
            return Err(http_error(
                "R_HTTP_REDIRECT",
                "redirect changed origin; credentials were not forwarded",
            ));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(http_error("R_HTTP_TIMEOUT", "HTTP request timed out"));
        }
        let response = client
            .request(method.clone(), url.clone())
            .headers(headers.clone())
            .timeout(remaining)
            .body(body.clone())
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    http_error("R_HTTP_TIMEOUT", "HTTP request timed out")
                } else {
                    http_error("R_HTTP_REQUEST", "HTTP request failed")
                }
            })?;

        let status = response.status();
        if matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
            if let Some(location) = response.headers().get(reqwest::header::LOCATION) {
                if redirect_count == policy.max_http_redirects {
                    return Err(http_error(
                        "R_HTTP_REDIRECT",
                        "HTTP redirect limit exceeded",
                    ));
                }
                let location = location.to_str().map_err(|_| {
                    http_error("R_HTTP_REDIRECT", "HTTP redirect location is invalid")
                })?;
                let next_url = url
                    .join(location)
                    .map_err(|_| http_error("R_HTTP_REDIRECT", "HTTP redirect URL is invalid"))?;
                let next_host = validate_http_url(&next_url, policy)
                    .map_err(|_| http_error("R_HTTP_REDIRECT", "redirect host is not granted"))?;
                if next_host != initial_host
                    || next_url.origin().ascii_serialization() != initial_origin
                {
                    return Err(http_error(
                        "R_HTTP_REDIRECT",
                        "redirect changed origin; credentials were not forwarded",
                    ));
                }
                if (status.as_u16() == 303 && method != reqwest::Method::HEAD)
                    || (matches!(status.as_u16(), 301 | 302) && method == reqwest::Method::POST)
                {
                    method = reqwest::Method::GET;
                    body.clear();
                    for name in [
                        "content-type",
                        "content-encoding",
                        "content-language",
                        "content-location",
                    ] {
                        headers.remove(name);
                    }
                }
                url = next_url;
                continue;
            }
        }

        if response
            .content_length()
            .is_some_and(|length| length > policy.max_http_response_bytes as u64)
        {
            return Err(http_error(
                "R_HTTP_LIMIT",
                "HTTP response exceeds the configured byte limit",
            ));
        }
        let response_headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_ascii_lowercase(), value.to_owned()))
            })
            .fold(
                BTreeMap::<String, String>::new(),
                |mut result, (name, value)| {
                    result
                        .entry(name)
                        .and_modify(|existing| {
                            existing.push_str(", ");
                            existing.push_str(&value);
                        })
                        .or_insert(value);
                    result
                },
            );
        let mut response = response;
        let reserve = response
            .content_length()
            .unwrap_or(0)
            .min(policy.max_http_response_bytes as u64)
            .min(64 * 1024) as usize;
        let mut bytes = Vec::with_capacity(reserve);
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            if error.is_timeout() {
                http_error("R_HTTP_TIMEOUT", "HTTP request timed out")
            } else {
                http_error("R_HTTP_REQUEST", "could not read HTTP response")
            }
        })? {
            if bytes.len().saturating_add(chunk.len()) > policy.max_http_response_bytes {
                return Err(http_error(
                    "R_HTTP_LIMIT",
                    "HTTP response exceeds the configured byte limit",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(HttpBytesResponse {
            status: status.as_u16(),
            headers: response_headers,
            body: bytes,
        });
    }
    Err(http_error(
        "R_HTTP_REDIRECT",
        "HTTP redirect limit exceeded",
    ))
}

struct HttpRequestBytes<'a> {
    method: &'a str,
    input: &'a str,
    headers: &'a BTreeMap<String, String>,
    body: &'a [u8],
    timeout: Duration,
}

fn perform_http_request_bytes(
    host: &HostHttpClient,
    policy: &HostPolicy,
    request: HttpRequestBytes<'_>,
    budget: &Budget,
) -> Result<HttpBytesResponse, HttpErrorValue> {
    let HttpRequestBytes {
        method,
        input,
        headers,
        body,
        timeout: request_timeout,
    } = request;
    let deadline = Instant::now().checked_add(request_timeout).ok_or_else(|| {
        http_error(
            "R_HTTP_TIMEOUT",
            "HTTP timeout is outside the supported range",
        )
    })?;
    host.executor.block_on(async {
        let request = execute_http_bytes(
            &host.client,
            policy,
            method,
            input,
            headers,
            body,
            deadline,
        );
        tokio::pin!(request);
        let cancellation = async {
            loop {
                if budget.is_cancelled() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        tokio::pin!(cancellation);
        tokio::select! {
            biased;
            _ = &mut cancellation => Err(http_error("R_HTTP_CANCELLED", "HTTP request was cancelled")),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                Err(http_error("R_HTTP_TIMEOUT", "HTTP request timed out"))
            },
            result = &mut request => result,
        }
    })
}

fn perform_http_get(
    host: &HostHttpClient,
    policy: &HostPolicy,
    input: &str,
    budget: &Budget,
) -> Result<String, HttpErrorValue> {
    let response = perform_http_request_bytes(
        host,
        policy,
        HttpRequestBytes {
            method: "GET",
            input,
            headers: &BTreeMap::new(),
            body: &[],
            timeout: policy.http_timeout,
        },
        budget,
    )?;
    if !(200..300).contains(&response.status) {
        return Err(http_error(
            "R_HTTP_STATUS",
            format!("HTTP server returned status {}", response.status),
        ));
    }
    if response.body.len() > policy.max_text_bytes {
        return Err(http_error(
            "R_HTTP_LIMIT",
            "HTTP response exceeds the configured text byte limit",
        ));
    }
    String::from_utf8(response.body)
        .map_err(|_| http_error("R_HTTP_ENCODING", "HTTP response body is not valid UTF-8"))
}

#[derive(Debug)]
enum JsonDecodeError {
    Parse(serde_json::Error),
    DuplicateKey {
        key: String,
        line: usize,
        column: usize,
    },
}

#[cfg(test)]
fn parse_unique_json(source: &str) -> Result<serde_json::Value, JsonDecodeError> {
    use serde::de::DeserializeSeed;

    let (safe_source, reserved_key) = escape_reserved_json_keys(source);
    let mut duplicate = None;
    let mut deserializer = serde_json::Deserializer::from_str(&safe_source);
    let parsed = UniqueJsonSeed {
        duplicate: &mut duplicate,
    }
    .deserialize(&mut deserializer);
    match parsed {
        Ok(mut value) => {
            deserializer.end().map_err(JsonDecodeError::Parse)?;
            if let Some(reserved_key) = reserved_key.as_deref() {
                restore_reserved_json_keys(&mut value, reserved_key);
            }
            Ok(value)
        }
        Err(error) => {
            if let Some(key) = duplicate.take() {
                let offset = duplicate_json_key(source)
                    .map(|(_, offset)| offset)
                    .unwrap_or(source.len());
                let (line, column) = source_position(source, offset);
                let key = if reserved_key.as_deref() == Some(key.as_str()) {
                    "$serde_json::private::Number".to_owned()
                } else {
                    key
                };
                Err(JsonDecodeError::DuplicateKey { key, line, column })
            } else {
                Err(JsonDecodeError::Parse(error))
            }
        }
    }
}

fn parse_unique_shared_json(source: &str) -> Result<SharedJson, JsonDecodeError> {
    use serde::de::DeserializeSeed;

    let (safe_source, reserved_key) = escape_reserved_json_keys(source);
    let mut duplicate = None;
    let mut deserializer = serde_json::Deserializer::from_str(&safe_source);
    let parsed = SharedJsonSeed {
        duplicate: &mut duplicate,
    }
    .deserialize(&mut deserializer);
    match parsed {
        Ok(mut value) => {
            deserializer.end().map_err(JsonDecodeError::Parse)?;
            if let Some(reserved_key) = reserved_key.as_deref() {
                restore_reserved_shared_json_keys(&mut value, reserved_key);
            }
            Ok(value)
        }
        Err(error) => {
            if let Some(key) = duplicate.take() {
                let offset = duplicate_json_key(source)
                    .map(|(_, offset)| offset)
                    .unwrap_or(source.len());
                let (line, column) = source_position(source, offset);
                let key = if reserved_key.as_deref() == Some(key.as_str()) {
                    "$serde_json::private::Number".to_owned()
                } else {
                    key
                };
                Err(JsonDecodeError::DuplicateKey { key, line, column })
            } else {
                Err(JsonDecodeError::Parse(error))
            }
        }
    }
}

fn restore_reserved_shared_json_keys(value: &mut SharedJson, reserved_key: &str) {
    match value {
        SharedJson::Array(values) => {
            for value in Shared::make_mut(values) {
                restore_reserved_shared_json_keys(Shared::make_mut(value), reserved_key);
            }
        }
        SharedJson::Object(values) => {
            let values = Shared::make_mut(values);
            if let Some(mut value) = values.remove(reserved_key) {
                restore_reserved_shared_json_keys(Shared::make_mut(&mut value), reserved_key);
                values.insert("$serde_json::private::Number".to_owned(), value);
            }
            for value in values.values_mut() {
                restore_reserved_shared_json_keys(Shared::make_mut(value), reserved_key);
            }
        }
        _ => {}
    }
}

pub(crate) fn escape_reserved_json_keys<'a>(source: &'a str) -> (Cow<'a, str>, Option<String>) {
    const RESERVED: &str = "$serde_json::private::Number";
    // The reserved key starts with `$` and can otherwise only appear through an escape.
    if memchr::memchr2(b'$', b'\\', source.as_bytes()).is_none() {
        return (Cow::Borrowed(source), None);
    }
    let bytes = source.as_bytes();
    let mut offset = 0;
    let mut has_reserved_key = false;
    while offset < bytes.len() {
        if bytes[offset] != b'"' {
            offset += 1;
            continue;
        }
        let start = offset;
        offset += 1;
        let mut escaped = false;
        while offset < bytes.len() {
            let byte = bytes[offset];
            offset += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                break;
            }
        }
        if offset > bytes.len() || bytes.get(offset - 1) != Some(&b'"') {
            break;
        }
        let mut after = offset;
        while bytes
            .get(after)
            .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        {
            after += 1;
        }
        let token = &source[start + 1..offset - 1];
        let is_reserved = if token.contains('\\') {
            serde_json::from_str::<String>(&source[start..offset])
                .is_ok_and(|decoded| decoded == RESERVED)
        } else {
            token == RESERVED
        };
        if bytes.get(after) == Some(&b':') && is_reserved {
            has_reserved_key = true;
            break;
        }
    }
    if !has_reserved_key {
        return (Cow::Borrowed(source), None);
    }

    let mut strings = Vec::<(usize, usize, String, bool)>::new();
    offset = 0;
    while offset < bytes.len() {
        if bytes[offset] != b'"' {
            offset += 1;
            continue;
        }
        let start = offset;
        offset += 1;
        let mut escaped = false;
        while offset < bytes.len() {
            let byte = bytes[offset];
            offset += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                break;
            }
        }
        if offset > bytes.len() || bytes.get(offset - 1) != Some(&b'"') {
            break;
        }
        let Ok(decoded) = serde_json::from_str::<String>(&source[start..offset]) else {
            continue;
        };
        let mut after = offset;
        while bytes
            .get(after)
            .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        {
            after += 1;
        }
        let is_key = bytes.get(after) == Some(&b':');
        strings.push((start, offset, decoded, is_key));
    }
    if !strings
        .iter()
        .any(|(_, _, value, is_key)| *is_key && value == RESERVED)
    {
        return (Cow::Borrowed(source), None);
    }
    let existing = strings
        .iter()
        .map(|(_, _, value, _)| value.as_str())
        .collect::<HashSet<_>>();
    let reserved_key = (0_u64..)
        .map(|number| {
            format!(
                "$guion_private_{number:0width$}$",
                width = RESERVED.len() - 16
            )
        })
        .find(|candidate| !existing.contains(candidate.as_str()))
        .expect("a unique private key can be generated");
    let mut rewritten = String::with_capacity(source.len());
    let mut copied_until = 0;
    for (start, end, decoded, is_key) in strings {
        if !is_key || decoded != RESERVED {
            continue;
        }
        let raw_content = &source[start + 1..end - 1];
        let escape_count = raw_content.len().saturating_sub(RESERVED.len()) / 5;
        let mut replacement = String::with_capacity(raw_content.len());
        for (index, character) in reserved_key.chars().enumerate() {
            if index < escape_count {
                replacement.push_str(&format!("\\u{:04x}", character as u32));
            } else {
                replacement.push(character);
            }
        }
        debug_assert_eq!(replacement.len(), raw_content.len());
        rewritten.push_str(&source[copied_until..start + 1]);
        rewritten.push_str(&replacement);
        copied_until = end - 1;
    }
    rewritten.push_str(&source[copied_until..]);
    (Cow::Owned(rewritten), Some(reserved_key))
}

pub(crate) fn restore_reserved_json_keys(value: &mut serde_json::Value, reserved_key: &str) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                restore_reserved_json_keys(value, reserved_key);
            }
        }
        serde_json::Value::Object(object) => {
            let Some(mut value) = object.remove(reserved_key) else {
                for value in object.values_mut() {
                    restore_reserved_json_keys(value, reserved_key);
                }
                return;
            };
            restore_reserved_json_keys(&mut value, reserved_key);
            object.insert("$serde_json::private::Number".to_owned(), value);
            for value in object.values_mut() {
                restore_reserved_json_keys(value, reserved_key);
            }
        }
        _ => {}
    }
}

struct UniqueJsonSeed<'duplicate> {
    duplicate: &'duplicate mut Option<String>,
}

struct SharedJsonSeed<'duplicate> {
    duplicate: &'duplicate mut Option<String>,
}

impl<'de> serde::de::DeserializeSeed<'de> for SharedJsonSeed<'_> {
    type Value = SharedJson;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(SharedJsonVisitor {
            duplicate: self.duplicate,
        })
    }
}

struct SharedJsonVisitor<'duplicate> {
    duplicate: &'duplicate mut Option<String>,
}

impl<'de> serde::de::Visitor<'de> for SharedJsonVisitor<'_> {
    type Value = SharedJson;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(SharedJson::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(SharedJson::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(SharedJson::Number(value.into()))
    }

    fn visit_i128<E>(self, value: i128) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_i128(value)
            .map(SharedJson::Number)
            .ok_or_else(|| E::custom("JSON integer is out of range"))
    }

    fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_u128(value)
            .map(SharedJson::Number)
            .ok_or_else(|| E::custom("JSON integer is out of range"))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(SharedJson::Number)
            .ok_or_else(|| E::custom("JSON number is out of range"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(SharedJson::String(SharedText::from(value)))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(SharedJson::String(SharedText::from(value)))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(SharedJson::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(SharedJson::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::DeserializeSeed;
        SharedJsonSeed {
            duplicate: &mut *self.duplicate,
        }
        .deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(SharedJsonSeed {
            duplicate: &mut *self.duplicate,
        })? {
            values.push(Shared::new(value));
        }
        Ok(SharedJson::Array(Shared::new(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut object = BTreeMap::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                *self.duplicate = Some(key);
                return Err(serde::de::Error::custom("duplicate JSON object key"));
            }
            let value = map.next_value_seed(SharedJsonSeed {
                duplicate: &mut *self.duplicate,
            })?;
            object.insert(key, Shared::new(value));
        }
        if object.len() == 1 {
            if let Some(SharedJson::String(number)) = object
                .get("$serde_json::private::Number")
                .map(Shared::as_ref)
            {
                if let Ok(number) = number.parse::<serde_json::Number>() {
                    return Ok(SharedJson::Number(number));
                }
            }
        }
        Ok(SharedJson::Object(Shared::new(object)))
    }
}

impl<'de> serde::de::DeserializeSeed<'de> for UniqueJsonSeed<'_> {
    type Value = serde_json::Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueJsonVisitor {
            duplicate: self.duplicate,
        })
    }
}

struct UniqueJsonVisitor<'duplicate> {
    duplicate: &'duplicate mut Option<String>,
}

impl<'de> serde::de::Visitor<'de> for UniqueJsonVisitor<'_> {
    type Value = serde_json::Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Number(value.into()))
    }

    fn visit_i128<E>(self, value: i128) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_i128(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| E::custom("JSON integer is out of range"))
    }

    fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_u128(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| E::custom("JSON integer is out of range"))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| E::custom("JSON number is out of range"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(serde_json::Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(serde_json::Value::String(value))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::DeserializeSeed;
        UniqueJsonSeed {
            duplicate: self.duplicate,
        }
        .deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(UniqueJsonSeed {
            duplicate: &mut *self.duplicate,
        })? {
            values.push(value);
        }
        Ok(serde_json::Value::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let Some(first_key) = map.next_key::<String>()? else {
            return Ok(serde_json::Value::Object(serde_json::Map::new()));
        };

        // serde_json uses this reserved key when arbitrary-precision numbers
        // cannot be represented by its machine-number visitor methods.
        let mut keys = HashSet::new();
        let mut object = serde_json::Map::new();
        let first_value = map.next_value_seed(UniqueJsonSeed {
            duplicate: &mut *self.duplicate,
        })?;
        let next_key = map.next_key::<String>()?;
        if first_key == "$serde_json::private::Number" && next_key.is_none() {
            if let serde_json::Value::String(number) = &first_value {
                if let Ok(number) = number.parse::<serde_json::Number>() {
                    return Ok(serde_json::Value::Number(number));
                }
            }
        }
        keys.insert(first_key.clone());
        object.insert(first_key, first_value);
        let mut key = next_key;
        while let Some(current_key) = key {
            if !keys.insert(current_key.clone()) {
                *self.duplicate = Some(current_key);
                return Err(serde::de::Error::custom("duplicate JSON object key"));
            }
            let value = map.next_value_seed(UniqueJsonSeed {
                duplicate: &mut *self.duplicate,
            })?;
            object.insert(current_key, value);
            key = map.next_key::<String>()?;
        }
        Ok(serde_json::Value::Object(object))
    }
}

fn duplicate_json_key(source: &str) -> Option<(String, usize)> {
    #[derive(Default)]
    struct ObjectKeys {
        expecting_key: bool,
        keys: HashSet<String>,
    }
    let bytes = source.as_bytes();
    let mut stack: Vec<Option<ObjectKeys>> = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        match bytes[offset] {
            b' ' | b'\n' | b'\r' | b'\t' | b':' => offset += 1,
            b'{' => {
                stack.push(Some(ObjectKeys {
                    expecting_key: true,
                    ..ObjectKeys::default()
                }));
                offset += 1;
            }
            b'[' => {
                stack.push(None);
                offset += 1;
            }
            b'}' | b']' => {
                stack.pop();
                offset += 1;
            }
            b',' => {
                if let Some(Some(object)) = stack.last_mut() {
                    object.expecting_key = true;
                }
                offset += 1;
            }
            b'"' => {
                let start = offset;
                offset += 1;
                while offset < bytes.len() {
                    match bytes[offset] {
                        b'\\' => offset = (offset + 2).min(bytes.len()),
                        b'"' => {
                            offset += 1;
                            break;
                        }
                        _ => offset += 1,
                    }
                }
                if let Some(Some(object)) = stack.last_mut() {
                    if object.expecting_key {
                        let key = serde_json::from_str::<String>(&source[start..offset]).ok()?;
                        if !object.keys.insert(key.clone()) {
                            return Some((key, start));
                        }
                        object.expecting_key = false;
                    }
                }
            }
            _ => offset += 1,
        }
    }
    None
}

#[cfg(test)]
mod unique_json_parser_tests {
    use super::{
        escape_reserved_json_keys, parse_unique_json, parse_unique_shared_json, JsonDecodeError,
    };

    #[test]
    fn does_not_copy_input_when_no_reserved_key_needs_rewriting() {
        let source = r#"{"rows":[{"value":42,"label":"東京"}]}"#;
        let (safe_source, reserved_key) = escape_reserved_json_keys(source);

        assert!(reserved_key.is_none());
        assert_eq!(safe_source, source);
        assert!(std::ptr::eq(source.as_ptr(), safe_source.as_ptr()));
    }

    #[test]
    fn rejects_escaped_duplicate_keys_with_the_duplicate_location() {
        let source = "{\n \"rows\": [{\"name\": \"first\", \"\\u006eame\": \"second\"}]\n}";
        match parse_unique_json(source) {
            Err(JsonDecodeError::DuplicateKey { key, line, column }) => {
                assert_eq!(key, "name");
                assert_eq!((line, column), (2, 29));
            }
            other => panic!("expected duplicate-key error, got {other:?}"),
        }
    }

    #[test]
    fn retains_arbitrary_precision_numbers_and_nested_objects() {
        let source = r#"{"rows":[{"value":123456789012345678901234567890,"label":"東京"}]}"#;
        let value = parse_unique_json(source).expect("valid JSON should parse");
        assert_eq!(
            value["rows"][0]["value"].to_string(),
            "123456789012345678901234567890"
        );
        assert_eq!(value["rows"][0]["label"], "東京");
    }

    #[test]
    fn accepts_user_objects_with_serde_private_number_key() {
        let source = r#"{"rows":[{"value":{"$serde_json::private::Number":{"nested":1}}}]}"#;
        let value = parse_unique_json(source).expect("reserved-looking object keys are valid JSON");
        assert_eq!(
            value["rows"][0]["value"]["$serde_json::private::Number"]["nested"],
            1
        );

        let numeric_text = r#"{"value":{"\u0024serde_json::private::Number":"123"}}"#;
        let value = parse_unique_json(numeric_text).expect("numeric strings remain strings");
        assert_eq!(value["value"]["$serde_json::private::Number"], "123");
    }

    #[test]
    fn reports_malformed_json_as_a_parser_error() {
        assert!(matches!(
            parse_unique_json("{\"value\": [}"),
            Err(JsonDecodeError::Parse(_))
        ));
    }

    #[test]
    fn shared_parser_matches_generic_parser_for_supported_json_values() {
        for source in [
            "null",
            "true",
            "-17",
            "1.25e3",
            r#"{"rows":[{"value":42,"label":"東京"}]}"#,
            r#"{"rows":[{"value":123456789012345678901234567890,"label":"東京"}]}"#,
            r#"{"value":{"$serde_json::private::Number":{"nested":1}}}"#,
            r#"{"value":{"\u0024serde_json::private::Number":"123"}}"#,
        ] {
            let generic = parse_unique_json(source).expect("generic JSON parser accepts fixture");
            let shared =
                parse_unique_shared_json(source).expect("shared JSON parser accepts fixture");

            assert_eq!(shared.to_serde(), generic, "source: {source}");
        }
    }

    #[test]
    fn shared_parser_rejects_escaped_duplicate_keys_at_the_same_location() {
        let source = "{\n \"rows\": [{\"name\": \"first\", \"\\u006eame\": \"second\"}]\n}";

        match parse_unique_shared_json(source) {
            Err(JsonDecodeError::DuplicateKey { key, line, column }) => {
                assert_eq!(key, "name");
                assert_eq!((line, column), (2, 29));
            }
            other => panic!("expected duplicate-key error, got {other:?}"),
        }
    }

    #[test]
    fn shared_parser_reports_malformed_json_as_a_parser_error() {
        assert!(matches!(
            parse_unique_shared_json("{\"value\": [}"),
            Err(JsonDecodeError::Parse(_))
        ));
    }
}

#[cfg(test)]
mod function_cache_tests {
    use super::share_artifact;
    use guion_model::{Artifact, Function, Type};
    use std::sync::Arc;

    #[test]
    fn frame_and_worker_owners_share_immutable_artifact_storage() {
        let function = Function {
            name: "work".into(),
            parameters: vec![],
            result: Type::Int,
            local_count: 0,
            code: vec![],
        };
        let artifact = Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "cache-test".into(),
            sources: vec![],
            entry: "work".into(),
            functions: vec![function],
            externals: vec![],
            records: vec![],
        };
        let shared = share_artifact(artifact);

        let first_frame = Arc::clone(&shared);
        let second_worker = Arc::clone(&shared);

        assert!(Arc::ptr_eq(&first_frame, &second_worker));
        assert_eq!(Arc::strong_count(&shared), 3);
    }
}

fn source_position(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset.min(source.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap_or("").len() + 1;
    (line, column)
}

fn atomic_replace_text(path: &Path, content: &str) -> std::io::Result<()> {
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "state".into());
    let permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());

    for _ in 0..32 {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".{name}.guion-tmp-{}-{id}", std::process::id()));
        let mut file = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };

        let replacement = (|| {
            file.write_all(content.as_bytes())?;
            if let Some(permissions) = &permissions {
                fs::set_permissions(&temporary, permissions.clone())?;
            }
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();
        if replacement.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return replacement;
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not reserve a unique temporary file name",
    ))
}

fn file_io_error_code(error: &std::io::Error, fallback: &'static str) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::NotFound => "R_FILE_NOT_FOUND",
        std::io::ErrorKind::PermissionDenied => "R_FILE_PERMISSION",
        _ => fallback,
    }
}

fn host_file_path(
    policy: &HostPolicy,
    requested: &str,
    write: bool,
    span: Span,
) -> Result<PathBuf, RuntimeError> {
    host_file_path_with_walk_root(policy, requested, write, span, false)
}

/// The directory entry `requested` names, for operations that act on the entry itself rather
/// than on what a symlink points to (`remove`, `rename`). The parent directory is resolved and
/// confined like a write; the final component is not followed.
fn host_file_entry_path(
    policy: &HostPolicy,
    requested: &str,
    span: Span,
) -> Result<PathBuf, RuntimeError> {
    let path = Path::new(requested);
    let Some(std::path::Component::Normal(name)) = path.components().next_back() else {
        return Err(RuntimeError::new(
            "R_HOST_DENIED",
            "file path must name an entry inside the granted project root",
            span,
        ));
    };
    let parent = path
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = if parent.is_empty() {
        ".".to_owned()
    } else {
        format!("{parent}/.")
    };
    Ok(host_file_path(policy, &parent, true, span)?.join(name))
}

fn host_file_path_no_follow(
    policy: &HostPolicy,
    requested: &str,
    span: Span,
) -> Result<PathBuf, RuntimeError> {
    host_file_path_with_walk_root(policy, requested, false, span, true)
}

fn host_file_path_with_walk_root(
    policy: &HostPolicy,
    requested: &str,
    write: bool,
    span: Span,
    preserve_walk_root: bool,
) -> Result<PathBuf, RuntimeError> {
    let allowed = if write {
        policy.allow_file_write
    } else {
        policy.allow_file_read
    };
    if !allowed {
        return Err(RuntimeError::new(
            "R_HOST_DENIED",
            if write {
                "file writing is not granted by the host policy"
            } else {
                "file reading is not granted by the host policy"
            },
            span,
        ));
    }
    let alias = policy
        .file_root_aliases
        .iter()
        .filter_map(|(alias, root)| {
            requested
                .strip_prefix(alias)
                .and_then(|suffix| suffix.strip_prefix('/'))
                .map(|relative| (alias, root, relative))
        })
        .max_by_key(|(alias, _, _)| alias.len());
    let (requested_path, roots) = if let Some((_, root, relative)) = alias {
        (Path::new(relative), vec![root])
    } else {
        let mut roots = Vec::with_capacity(1 + policy.additional_file_roots.len());
        if let Some(root) = &policy.file_root {
            roots.push(root);
        }
        roots.extend(policy.additional_file_roots.iter());
        (Path::new(requested), roots)
    };
    if roots.is_empty() {
        return Err(RuntimeError::new(
            "R_HOST_DENIED",
            "file access has no configured project root",
            span,
        ));
    }
    let roots = roots
        .into_iter()
        .map(|root| {
            root.canonicalize().map_err(|error| {
                RuntimeError::new(
                    "R_HOST_DENIED",
                    format!("cannot resolve file capability root: {error}"),
                    span,
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if requested_path.is_absolute() {
        return Err(RuntimeError::new(
            "R_HOST_DENIED",
            "file paths must be relative to the granted project root",
            span,
        ));
    }
    if requested_path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        return Err(RuntimeError::new(
            "R_HOST_DENIED",
            "file path escapes or is absolute relative to the granted project root",
            span,
        ));
    }

    if !write {
        let mut missing_error = None;
        for root in &roots {
            if preserve_walk_root {
                let candidate = root.join(requested_path);
                let components = requested_path
                    .components()
                    .filter(|component| matches!(component, std::path::Component::Normal(_)))
                    .collect::<Vec<_>>();
                let mut current = root.clone();
                let mut failure = None;
                let mut candidate_missing = false;
                for (index, component) in components.iter().enumerate() {
                    current.push(component);
                    match fs::symlink_metadata(&current) {
                        Ok(metadata)
                            if metadata.file_type().is_symlink()
                                && index + 1 < components.len() =>
                        {
                            failure = Some(RuntimeError::new(
                                "R_HOST_DENIED",
                                "fs_walk root cannot pass through a symlink directory",
                                span,
                            ));
                            break;
                        }
                        Ok(_) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            missing_error = Some(error);
                            candidate_missing = true;
                            break;
                        }
                        Err(error) => {
                            failure = Some(RuntimeError::new(
                                file_io_error_code(&error, "R_FILE_READ"),
                                format!("cannot inspect `{requested}`: {error}"),
                                span,
                            ));
                            break;
                        }
                    }
                }
                if let Some(error) = failure {
                    return Err(error);
                }
                if candidate_missing {
                    continue;
                }
                return Ok(candidate);
            }
            match root.join(requested_path).canonicalize() {
                Ok(resolved) if resolved.starts_with(root) => return Ok(resolved),
                Ok(_) => {
                    return Err(RuntimeError::new(
                        "R_HOST_DENIED",
                        "file path escapes the granted project root",
                        span,
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    missing_error = Some(error);
                }
                Err(error) => {
                    return Err(RuntimeError::new(
                        file_io_error_code(&error, "R_FILE_READ"),
                        format!("cannot resolve `{requested:?}`: {error}"),
                        span,
                    ));
                }
            }
        }
        let error = missing_error.unwrap_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no granted root contains this path",
            )
        });
        return Err(RuntimeError::new(
            file_io_error_code(&error, "R_FILE_READ"),
            format!("cannot resolve `{requested:?}`: {error}"),
            span,
        ));
    }

    let root = roots.last().expect("at least one file root was checked");
    let candidate = root.join(requested_path);
    // A dangling symlink does not "exist", but writing through it would create its target,
    // which may lie outside the root.
    if !candidate.exists()
        && fs::symlink_metadata(&candidate).is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(RuntimeError::new(
            "R_HOST_DENIED",
            "cannot write through a symlink whose target does not exist",
            span,
        ));
    }
    let resolved = if !candidate.exists() {
        let name = candidate.file_name().ok_or_else(|| {
            RuntimeError::new("R_HOST_DENIED", "file path must name a file", span)
        })?;
        let parent = candidate
            .parent()
            .unwrap_or(root)
            .canonicalize()
            .map_err(|error| {
                RuntimeError::new(
                    "R_FILE_WRITE",
                    format!("cannot resolve parent directory: {error}"),
                    span,
                )
            })?;
        if !parent.starts_with(root) {
            return Err(RuntimeError::new(
                "R_HOST_DENIED",
                "file path escapes the granted project root",
                span,
            ));
        }
        parent.join(name)
    } else {
        candidate.canonicalize().map_err(|error| {
            RuntimeError::new(
                file_io_error_code(&error, "R_FILE_WRITE"),
                format!("cannot resolve `{requested:?}`: {error}"),
                span,
            )
        })?
    };
    if !resolved.starts_with(root) {
        return Err(RuntimeError::new(
            "R_HOST_DENIED",
            "file path escapes the granted project root",
            span,
        ));
    }
    Ok(resolved)
}

#[cfg(test)]
mod native_small_list_tests {
    use super::{
        native_list_set, native_make_list, Budget, CancellationToken, NativeFloatList,
        NativeIntList, NativeListArena, RuntimeProfileCounters, NATIVE_LIST_FLOAT_KIND,
        NATIVE_LIST_INT_KIND,
    };
    use std::{slice, sync::Arc};

    fn arena() -> (Arc<Budget>, NativeListArena) {
        let budget = Arc::new(Budget::new(
            1_000_000,
            1_000_000,
            CancellationToken::new(),
            None,
        ));
        let arena = NativeListArena::new(Arc::as_ptr(&budget), 10_000, 10_000, 0, None);
        (budget, arena)
    }

    #[test]
    fn small_integer_list_copy_uses_inline_data_and_preserves_the_original() {
        let (_budget, mut arena) = arena();
        let values = [1_i64, 2, 3, 4, 5, 6, 7, 8];
        let original = unsafe {
            native_make_list(
                &mut arena,
                values.as_ptr(),
                values.len() as i64,
                NATIVE_LIST_INT_KIND,
                0,
            )
        };
        assert!(!original.is_null());
        assert!(
            arena.int_storage.is_empty(),
            "small list should use inline data"
        );
        let updated =
            unsafe { native_list_set(&mut arena, original, 0, 9, NATIVE_LIST_INT_KIND, 1) };
        assert!(!updated.is_null());
        let original = unsafe { &*original.cast::<NativeIntList>() };
        let updated = unsafe { &*updated.cast::<NativeIntList>() };
        assert_eq!(
            unsafe { slice::from_raw_parts(original.data, original.len) },
            &values
        );
        assert_eq!(
            unsafe { slice::from_raw_parts(updated.data, updated.len) },
            &[9, 2, 3, 4, 5, 6, 7, 8]
        );
        assert!(
            arena.int_storage.is_empty(),
            "small copies should stay inline"
        );
    }

    #[test]
    fn small_float_list_copy_uses_inline_data_and_preserves_the_original() {
        let (_budget, mut arena) = arena();
        let values = [1.5_f64.to_bits() as i64, 2.5_f64.to_bits() as i64];
        let original = unsafe {
            native_make_list(
                &mut arena,
                values.as_ptr(),
                values.len() as i64,
                NATIVE_LIST_FLOAT_KIND,
                0,
            )
        };
        assert!(!original.is_null());
        assert!(
            arena.float_storage.is_empty(),
            "small list should use inline data"
        );
        let replacement = 9.5_f64.to_bits() as i64;
        let updated = unsafe {
            native_list_set(
                &mut arena,
                original,
                1,
                replacement,
                NATIVE_LIST_FLOAT_KIND,
                1,
            )
        };
        assert!(!updated.is_null());
        let original = unsafe { &*original.cast::<NativeFloatList>() };
        let updated = unsafe { &*updated.cast::<NativeFloatList>() };
        assert_eq!(
            unsafe { slice::from_raw_parts(original.data, original.len) },
            &[values[0] as u64, values[1] as u64]
        );
        assert_eq!(
            unsafe { slice::from_raw_parts(updated.data, updated.len) },
            &[values[0] as u64, replacement as u64]
        );
        assert!(
            arena.float_storage.is_empty(),
            "small copies should stay inline"
        );
    }

    #[test]
    fn large_native_lists_keep_external_storage() {
        let (_budget, mut arena) = arena();
        let values = [1_i64, 2, 3, 4, 5, 6, 7, 8, 9];
        let list = unsafe {
            native_make_list(
                &mut arena,
                values.as_ptr(),
                values.len() as i64,
                NATIVE_LIST_INT_KIND,
                0,
            )
        };
        assert!(!list.is_null());
        assert_eq!(arena.int_storage.len(), 1);
    }

    #[test]
    fn invalid_native_list_set_attempts_do_not_increment_the_operation_counter() {
        let budget = Arc::new(Budget::new(
            1_000_000,
            1_000_000,
            CancellationToken::new(),
            None,
        ));
        let profile = Arc::new(RuntimeProfileCounters::default());
        let mut arena = NativeListArena::new(
            Arc::as_ptr(&budget),
            10_000,
            10_000,
            0,
            Some(profile.clone()),
        );
        let values = [1_i64, 2];
        let list = unsafe {
            native_make_list(
                &mut arena,
                values.as_ptr(),
                values.len() as i64,
                NATIVE_LIST_INT_KIND,
                0,
            )
        };

        let invalid = unsafe { native_list_set(&mut arena, list, 2, 9, NATIVE_LIST_INT_KIND, 1) };

        assert!(invalid.is_null());
        assert_eq!(profile.to_json()["list_set"]["operations"], 0);
    }
}

#[cfg(test)]
thread_local! {
    static ARTIFACT_SCAN_TALLY: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod native_entry_cost_tests {
    use super::*;

    /// A small native-eligible helper called from an interpreted loop must not rescan the whole
    /// artifact for text constants on every entry.
    #[test]
    fn native_entry_does_not_rescan_the_artifact_per_call() {
        let source = r#"fn chunk_rows() -> Int:
    return 65536

record Row:
    fields: List[Text]
    line: Int

fn main() -> Int:
    var rows: List[Row] = []
    var seen = 0
    while len(rows) < chunk_rows() and seen < 3000:
        set rows = list_push(rows, Row(["a", "b"], seen))
        set seen = seen + 1
    return len(rows)
"#;
        let artifact = guion_compiler::compile("entry-cost.gu", source).expect("compiles");
        let mut instance = Runtime::new()
            .load(artifact, HostPolicy::default())
            .expect("loads");
        ARTIFACT_SCAN_TALLY.with(|scans| scans.set(0));
        instance.invoke("main", vec![]).expect("runs");
        let scans = ARTIFACT_SCAN_TALLY.with(|scans| scans.get());
        assert!(
            scans <= 2,
            "artifact scanned {scans} times for 3000 helper calls"
        );
    }
}

#[cfg(test)]
mod runtime_profile_histogram_tests {
    use super::RuntimeProfileCounters;

    #[test]
    fn copy_size_histogram_tracks_bucket_boundaries_and_sums_to_total() {
        let profile = RuntimeProfileCounters::default();
        for (elements, bytes) in [(0, 1), (8, 2), (9, 3), (32, 4), (33, 5), (128, 6), (129, 7)] {
            profile.record_list_set_copy(elements, bytes);
        }

        let list_set = profile.to_json()["list_set"].clone();
        let histogram = &list_set["copy_size_histogram"]["buckets"];
        assert_eq!(histogram["0-8"]["copies"], 2);
        assert_eq!(histogram["0-8"]["estimated_copy_bytes"], 3);
        assert_eq!(histogram["9-32"]["copies"], 2);
        assert_eq!(histogram["9-32"]["estimated_copy_bytes"], 7);
        assert_eq!(histogram["33-128"]["copies"], 2);
        assert_eq!(histogram["33-128"]["estimated_copy_bytes"], 11);
        assert_eq!(histogram["129+"]["copies"], 1);
        assert_eq!(histogram["129+"]["estimated_copy_bytes"], 7);

        let total: u64 = ["0-8", "9-32", "33-128", "129+"]
            .into_iter()
            .map(|bucket| histogram[bucket]["copies"].as_u64().unwrap())
            .sum();
        assert_eq!(total, list_set["copy_on_write_copies"].as_u64().unwrap());
    }
}

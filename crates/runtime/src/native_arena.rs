//! Address-stable list and tagged-value storage shared with JIT code.

use super::*;

// JIT code keeps raw pointers to list descriptors and tagged values. Boxes and
// inline slots plus fixed-capacity overflow chunks keep their addresses stable.
#[allow(clippy::vec_box)]
/// Boxed descriptors with stable addresses and constant-time address membership.
pub(crate) struct AddressedBoxes<T> {
    pub(crate) items: Vec<Box<T>>,
    pub(crate) addresses: std::collections::HashSet<usize, std::hash::BuildHasherDefault<AddressHasher>>,
}

#[derive(Default)]
pub(crate) struct AddressHasher(u64);

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
    pub(crate) fn new() -> Self {
        Self {
            items: Vec::new(),
            addresses: Default::default(),
        }
    }

    pub(crate) fn try_reserve(&mut self, additional: usize) -> Result<(), std::collections::TryReserveError> {
        self.items.try_reserve(additional)?;
        self.addresses.try_reserve(additional)
    }

    pub(crate) fn push(&mut self, item: Box<T>) {
        self.addresses.insert((&*item as *const T) as usize);
        self.items.push(item);
    }

    pub(crate) fn last_mut(&mut self) -> Option<&mut Box<T>> {
        self.items.last_mut()
    }

    pub(crate) fn clear(&mut self) {
        self.items.clear();
        self.addresses.clear();
    }

    pub(crate) fn find_address(&self, address: usize) -> Option<&T> {
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

pub(crate) struct NativeListArena {
    pub(crate) float_lists: AddressedBoxes<NativeFloatList>,
    pub(crate) float_storage: Vec<Vec<Float64>>,
    pub(crate) int_lists: AddressedBoxes<NativeIntList>,
    pub(crate) inline_int_lists: Vec<Vec<NativeIntList>>,
    pub(crate) int_storage: Vec<Vec<i64>>,
    pub(crate) handle_lists: AddressedBoxes<NativeHandleList>,
    pub(crate) handle_storage: Vec<Vec<i64>>,
    // Native code holds raw pointers to each descriptor, so each needs a stable address.
    #[allow(clippy::vec_box)]
    pub(crate) native_records: Vec<Box<NativeRecord>>,
    pub(crate) native_record_types: Vec<usize>,
    pub(crate) native_record_fields: Vec<Box<[i64]>>,
    pub(crate) native_record_indices: HashMap<i64, usize>,
    pub(crate) values: Vec<Value>,
    pub(crate) constant_values: Arc<Vec<Value>>,
    pub(crate) inline_tagged_values: [NativeTaggedValue; NATIVE_INLINE_TAGGED_VALUES],
    pub(crate) inline_tagged_value_len: usize,
    pub(crate) tagged_values: Vec<Vec<NativeTaggedValue>>,
    pub(crate) tagged_chunk_starts: std::collections::BTreeMap<usize, usize>,
    pub(crate) native_vm_continuations: Vec<NativeVmContinuationFrame>,
    pub(crate) runtime_context: Option<NativeRuntimeContext>,
    pub(crate) host_error: Option<RuntimeError>,
    pub(crate) budget: *const Budget,
    pub(crate) profile: Option<Arc<RuntimeProfileCounters>>,
    pub(crate) max_collection_items: usize,
    pub(crate) charged_bytes: usize,
    pub(crate) output_limit_bytes: usize,
    pub(crate) initial_output_bytes: usize,
    pub(crate) committed_output_bytes: usize,
    pub(crate) pending_output: String,
    pub(crate) pending_print_bytes: Vec<usize>,
    pub(crate) reclaim_floor: usize,
    pub(crate) reclamations: u64,
}

const NATIVE_INLINE_TAGGED_VALUES: usize = 8;
/// Arena entries that must accumulate before a loop safepoint attempts to retire dead handles.
const NATIVE_ARENA_RECLAIM_MIN_ENTRIES: usize = 2048;
/// List items an entry must copy into a fresh native frame before its cost is tracked.
pub(crate) const LIST_ITEMS_TRACKED_AT_ENTRY: usize = 4096;
/// Copy-dominated large entries after which VM callers run the function in the VM.
pub(crate) const COPY_DOMINATED_ENTRY_LIMIT: u8 = 8;
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
pub(crate) struct NativeRuntimeContext {
    pub(crate) instance: *mut Instance,
    pub(crate) artifact: Arc<Artifact>,
    pub(crate) budget: Arc<Budget>,
    pub(crate) limits: VmLimits,
}

#[derive(Clone, Copy)]
pub(crate) struct NativeOperationRequest<'a> {
    pub(crate) operation: i64,
    pub(crate) function_index: i64,
    pub(crate) instruction_index: i64,
    pub(crate) depth: i64,
    pub(crate) arguments: &'a [i64],
    pub(crate) argument_tags: &'a [i64],
    pub(crate) result_tag: i64,
}

#[derive(Clone, Copy)]
pub(crate) struct NativeJsonOperationRequest<'a> {
    pub(crate) operation: i64,
    pub(crate) arguments: &'a [i64],
    pub(crate) argument_tags: &'a [i64],
    pub(crate) result_tag: i64,
    pub(crate) span: Span,
    pub(crate) operation_name: &'static str,
    pub(crate) runtime: &'a NativeRuntimeContext,
}

impl NativeListArena {
    pub(crate) fn new(
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

    pub(crate) fn charge(&mut self, bytes: usize) -> bool {
        if self.budget.is_null() || !unsafe { (*self.budget).allocate(bytes) } {
            return false;
        }
        self.charged_bytes = self.charged_bytes.saturating_add(bytes);
        true
    }

    pub(crate) fn retained_entries(&self) -> usize {
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

    pub(crate) fn discard_arena_handles(&mut self) {
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
    pub(crate) fn reclaim_dead_handles(&mut self, roots: &mut [[i64; 3]]) -> bool {
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

    pub(crate) fn charge_or_error(&mut self, bytes: usize, span: Span) -> bool {
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

    pub(crate) fn store_value(&mut self, value: Value) -> Option<i64> {
        self.values.try_reserve(1).ok()?;
        // These are backend handle-table entries. Their producer has already
        // charged any guest-visible value allocation; charging the handle slot
        // here would make the quota depend on whether the VM or JIT ran it.
        self.values.push(value);
        i64::try_from(self.values.len()).ok()
    }

    pub(crate) fn value(&self, handle: i64) -> Option<&Value> {
        let index = usize::try_from(handle.checked_sub(1)?).ok()?;
        self.values.get(index)
    }

    pub(crate) fn constant_value(&self, handle: i64) -> Option<&Value> {
        let index = usize::try_from(handle.checked_sub(NATIVE_CONSTANT_HANDLE_BASE)?).ok()?;
        self.constant_values.get(index)
    }

    pub(crate) fn handle_list(&self, handle: i64) -> Option<&NativeHandleList> {
        let address = usize::try_from(handle).ok()?;
        self.handle_lists.find_address(address)
    }

    pub(crate) fn native_record(&self, handle: i64) -> Option<(&NativeRecord, usize)> {
        let index = *self.native_record_indices.get(&handle)?;
        Some((
            self.native_records.get(index)?.as_ref(),
            *self.native_record_types.get(index)?,
        ))
    }

    pub(crate) fn float_list(&self, handle: i64) -> Option<&NativeFloatList> {
        let address = usize::try_from(handle).ok()?;
        self.float_lists.find_address(address)
    }

    pub(crate) fn int_list(&self, handle: i64) -> Option<&NativeIntList> {
        let address = usize::try_from(handle).ok()?;
        self.int_lists.find_address(address)
    }

    pub(crate) fn store_native_record_value(
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

    pub(crate) fn materialize_native_record(&self, handle: i64) -> Option<Value> {
        let (descriptor, record_index) = self.native_record(handle)?;
        self.materialize_native_record_descriptor(descriptor, record_index)
    }

    pub(crate) fn materialize_native_record_pointer(
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

    pub(crate) fn materialize_native_record_descriptor(
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

    pub(crate) fn tagged_value(&self, handle: i64) -> Option<&NativeTaggedValue> {
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

    pub(crate) fn tagged_clone_charge_bytes(&mut self, handle: i64) -> Option<usize> {
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

    pub(crate) fn native_value_handle_exists(&self, handle: i64) -> bool {
        self.constant_value(handle).is_some()
            || self.value(handle).is_some()
            || self.handle_list(handle).is_some()
            || self.native_record(handle).is_some()
            || self.tagged_value(handle).is_some()
    }

    pub(crate) fn ensure_tagged_capacity(&mut self) -> Option<()> {
        self.ensure_tagged_capacity_for(1)
    }

    pub(crate) fn ensure_tagged_capacity_for(&mut self, additional: usize) -> Option<()> {
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

    pub(crate) fn store_tagged_descriptor(&mut self, value: NativeTaggedValue) -> Option<i64> {
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

    pub(crate) fn encode_native_value(&mut self, value: Value) -> Option<i64> {
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

    pub(crate) fn store_tagged_value(&mut self, kind: i64, payload: Option<Value>) -> Option<i64> {
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

    pub(crate) fn store_native_tagged_value(
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

    pub(crate) fn store_native_arguments(
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

    /// Stores a list read from a record field as the list descriptor native code expects.
    /// Item types without a descriptor encoding return `None`, and the VM replays the read.
    /// The caller has already charged for the list.
    pub(crate) fn store_native_list_field(&mut self, list: &Value, item: &Type) -> Option<i64> {
        let items: Vec<Value> = match list {
            Value::List(items) => items.as_ref().clone(),
            Value::Float64List(items) => items.iter().copied().map(Value::Float64).collect(),
            _ => return None,
        };
        if items.len() > self.max_collection_items {
            return None;
        }
        match item {
            Type::Float64 => {
                let values = items
                    .iter()
                    .map(|value| match value {
                        Value::Float64(value) => Some(*value),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()?;
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
                i64::try_from(address).ok()
            }
            Type::Int | Type::Bool => {
                let values = items
                    .iter()
                    .map(|value| match (value, item) {
                        (Value::Int(value), Type::Int) => value.to_i64(),
                        (Value::Bool(value), Type::Bool) => Some(i64::from(*value)),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()?;
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
                i64::try_from(address).ok()
            }
            Type::Text => {
                let texts = items
                    .iter()
                    .map(|value| match value {
                        Value::Text(text) => Some(SharedText::clone(text)),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()?;
                self.store_native_text_list_charged(texts, false)
            }
            Type::Record(_) => {
                let mut encoded = Vec::new();
                encoded.try_reserve_exact(items.len()).ok()?;
                for value in items {
                    if !matches!(value, Value::Record(_, _)) {
                        return None;
                    }
                    encoded.push(self.encode_native_value(value)?);
                }
                self.handle_lists.try_reserve(1).ok()?;
                let inline_storage = encoded.len() <= NATIVE_INLINE_LIST_ITEMS;
                if !inline_storage {
                    self.handle_storage.try_reserve(1).ok()?;
                }
                let mut descriptor = Box::new(NativeHandleList {
                    data: std::ptr::null_mut(),
                    len: encoded.len(),
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
                    descriptor.inline[..encoded.len()].copy_from_slice(&encoded);
                    descriptor.data = descriptor.inline.as_mut_ptr();
                } else {
                    descriptor.data = encoded.as_mut_ptr();
                    self.handle_storage.push(encoded);
                }
                self.handle_lists.push(descriptor);
                let descriptor = self.handle_lists.last_mut()?;
                i64::try_from((&mut **descriptor as *mut NativeHandleList) as usize).ok()
            }
            _ => None,
        }
    }

    pub(crate) fn store_native_text_list(&mut self, items: Vec<SharedText>) -> Option<i64> {
        self.store_native_text_list_charged(items, true)
    }

    /// `charge` is false only when re-storing a list whose allocation was already charged.
    pub(crate) fn store_native_text_list_charged(
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

    pub(crate) fn materialize_native_value(&self, handle: i64) -> Option<Value> {
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

    pub(crate) fn decode_continuation_value(&self, raw: i64, tag: i64) -> Option<Value> {
        match tag {
            0 => Some(Value::Unit),
            1 => Some(Value::Int(BigInt::parse(&raw.to_string()).ok()?)),
            2 => Some(Value::Float64(Float64::new(f64::from_bits(raw as u64))?)),
            3 => Some(Value::Bool(raw != 0)),
            4 => self.materialize_native_value(raw),
            _ => None,
        }
    }

    pub(crate) fn append_native_continuation(
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

    pub(crate) fn bridge_handle_argument<'a>(
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

    pub(crate) fn decode_bridge_arguments(
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

    pub(crate) fn attach_runtime_context(
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

    pub(crate) fn execute_native_operation(&mut self, request: NativeOperationRequest<'_>) -> Option<i64> {
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
                // Machine code dereferences a list as a descriptor, so a list field must come
                // back as one, not as an arena handle.
                if let Type::List(item) = &definition.fields.get(*field_index)?.ty {
                    if result_tag != 4 {
                        return None;
                    }
                    return self.store_native_list_field(&field, item);
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

    pub(crate) fn execute_native_json_operation(
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

    pub(crate) fn execute_native_file_next_line(
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

    pub(crate) fn decode_bridge_value(&self, value: i64, tag: i64) -> Option<Value> {
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

    pub(crate) fn encode_bridge_value(&mut self, value: Value, tag: i64) -> Option<i64> {
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

    pub(crate) fn record_bridge_panic(&mut self, function_index: i64, instruction_index: i64) {
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

    pub(crate) fn allocate_inline_int(
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

    pub(crate) fn record_list_set_operation(&self) {
        if let Some(profile) = &self.profile {
            profile.record_list_set_operation();
        }
    }

    pub(crate) fn record_list_set_in_place_update(&self) {
        if let Some(profile) = &self.profile {
            profile.record_list_set_in_place_update();
        }
    }

    pub(crate) fn record_list_set_copy(&self, elements: usize, bytes: usize) {
        if let Some(profile) = &self.profile {
            profile.record_list_set_copy(elements, bytes);
        }
    }

    pub(crate) fn record_record_charge(&self, bytes: usize) {
        if let Some(profile) = &self.profile {
            profile.record_record_charge(bytes);
        }
    }

    pub(crate) fn record_guest_conditional_branch(&self) {
        if let Some(profile) = &self.profile {
            profile.record_guest_conditional_branch();
        }
    }

    pub(crate) fn print(&mut self, value: i64, kind: i64) -> bool {
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

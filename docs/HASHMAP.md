# rlox — Hash Table (`hash_map.rs`): How It Should Work & Current Gaps

This document specifies how `Table<T>` is *supposed* to behave, then lists where the current
implementation diverges. It's both a design reference and a bug map. Bugs are cross-listed in
[`tasks.md`](../tasks.md).

---

## 1. Purpose

`Table<T>` is the VM's string-keyed map. Two uses:
- **Global variables** — name → value (`vm.rs` `globals`).
- **String interning** — a set of unique strings so identical string literals are stored once
  (`compiler.rs` / `vm.rs` `table`).

Keys are `FatPointer` (raw pointer + byte length + cached 32-bit hash, from `common.rs`).
Values are the generic `T`.

---

## 2. Design: open addressing with linear probing

Every entry lives **directly in a flat bucket array** (`entries: Vec<Entry<T>>`), not in per-bucket
linked lists (that alternative is *chaining*). On a collision we **probe**: walk to the next slot,
`(i + 1) % capacity`, wrapping at the end, until we resolve the operation.

A bucket is in one of three states:

| `Entry` state | meaning | lookup behavior | insert behavior |
|---|---|---|---|
| `Occupied(key, val)` | live entry | compare key; stop if it matches | overwrite if key matches |
| `Vacant` | never used | **stop** — key is definitively absent | insert here |
| `TombStone` | was occupied, deleted | **skip, keep probing** | may reuse this slot |

### Why tombstones exist
Under linear probing a lookup stops at the first `Vacant` slot, because an empty slot proves the
key can't be further down the chain. If deletion blanked a slot to `Vacant`, it could sever a probe
chain and hide keys inserted after the deleted one. So deletion writes a `TombStone` (“keep
probing”) instead. Tombstones are an open-addressing concept — chaining wouldn't need them.

---

## 3. How each operation should work

### Probe (the shared primitive)
Walking from `start = hash % capacity`, for a **lookup**:
```
loop over slots from start, stepping (i+1) % capacity:
    Occupied(k, _) and k matches key  -> found
    Vacant                            -> not found (stop)
    TombStone                         -> keep going
    Occupied(k, _) and k != key       -> keep going
```

For an **insert**, the walk is subtly different and must:
```
remember `first_tombstone = None`
loop:
    Occupied(k, _) and k matches key  -> overwrite in place; done
    Occupied(k, _) and k != key       -> keep going
    TombStone                         -> if first_tombstone is None, record this index; keep going
    Vacant                            -> key is absent:
                                         insert at first_tombstone if set, else here; done
```
Two things this guarantees:
1. **No duplicates** — insert keeps scanning past tombstones to confirm the key isn't already
   present further along before placing a new copy.
2. **Tombstone reuse** — a new key fills the earliest tombstone instead of always taking a fresh
   `Vacant`, so tombstones don't accumulate and lengthen chains forever.

### `insert(key, value) -> bool`
- Ensure capacity first (may resize).
- Run the insert-probe above.
- Return `true` if it overwrote an existing key, `false` if it added a new one.
- Increment `size` **only** when a new key was added (never on overwrite).

### `get(key) / get_mut(key) -> Option<&T> / Option<&mut T>`
- Run the lookup-probe. Return `Some(value)` on a match, `None` on `Vacant` (absent).
- Must **never panic** on an absent key.

### `delete(key) -> Option<T>`
- Lookup the key. If found, return its value and replace the slot with `TombStone` (not `Vacant`).

### `ensure_capacity` (resize)
- Grow **before** the table gets too full — when `used * 100 >= capacity * load_factor` (e.g. 70%).
  Growing early keeps probe chains short.
- Growing = allocate a bigger array and **re-insert every `Occupied` entry** (their bucket depends
  on `capacity`, so it changes). Rehashing naturally drops all tombstones.
- The table must resize before it can become 100% full, or a probe for an absent key loops forever.

---

## 4. Key equality & the interning contract (the important invariant)

The table must use **one consistent notion of key equality** in every probe path (insert, lookup,
delete). Two valid designs:

### A — clox model (recommended here): intern everything, compare by pointer

This is two coupled changes:

1. **Intern *every* string, including computed ones.** Today `concat` allocates a fresh buffer and
   returns a new `FatPointer` *without* putting it in the intern table — so `"a" + "b"` gets a brand
   new pointer even if `"ab"` already exists. "Intern the result" means `concat` must mirror the
   compiler's `string()` flow: **look the string up first** (`get_existing_string` →
   `table.find_entry_with_value`, a content lookup) — if it's already there, **reuse that pointer**
   (`reuse_existing_string`); only on a miss allocate + insert (`create_new_string`). Note
   `create_new_string` is just the *miss* half — it always allocates and inserts, so calling it
   without the lookup would itself create a duplicate. After this there is exactly **one pointer per
   distinct string content**, regardless of how the string was produced.
2. **Compare keys by pointer identity everywhere.** Once every string is interned, "same content"
   and "same pointer" are the *same thing*, so all equality/probe paths (`is_occupied`,
   `find_entry_index`, and `Value`/`Obj` equality) can compare the raw `ptr` — a single cheap
   integer comparison — instead of the current mix of pointer / ptr+size+hash / byte-content checks.

**Why this closes two bugs at once:**
- *Gap #2 (inconsistent matching):* the paths disagree today only because a non-interned string
  could have a different pointer for identical content. With universal interning, pointer comparison
  *is* content comparison, so unifying on pointer identity is both consistent **and** correct.
- *Computed-string equality* (`"a" + "b" == "ab"` is `false` today, see §5 "Related"): it's false
  because `concat` gives `"ab"` a fresh pointer and `==` compares pointers. Once `concat` interns,
  both `"ab"`s share one pointer and `==` returns `true`.

**Cost / discipline:** interning must be *airtight* — if any code path ever creates a string key
without interning it, pointer comparison silently breaks (a content-equal key compares unequal). A
is fastest but depends on "every string is interned, always."

### B — content equality: compare by bytes, interning optional

Compare keys by `hash == hash && size == size && bytes_equal(...)`. The table is then self-correct
regardless of interning; interning becomes just a storage optimization. Slightly slower comparisons
(a byte compare on hash+size collisions), but there's no hidden invariant to maintain — a stray
un-interned key can't corrupt it. Pick B if you don't want to guarantee airtight interning.

Whichever is chosen, all paths must agree. Mixing them (pointer in one path, content in another) is
only accidentally correct while interning happens to hold — which is exactly today's latent bug.

---

## 5. Current gaps (implementation vs. this spec)

### ✅ Already fixed
- `insert` no longer increments `size` on overwrite (guarded by `if !new_value`).
- `get` no longer panics on an absent key (matches the `Option` instead of `.unwrap()`).
- `get_mut` no longer panics on an absent key (same `Option`-match fix).
- Lookup correctly probes **past** tombstones (verified by `get_probes_past_tombstone`;
  `find_entry_index` continues on `TombStone`).
- `insert` counts only new keys toward `size`, and `delete` decrements it, so `size` tracks live
  entries.
- Load factor now applies: `ensure_capacity` cross-multiplies
  `(size + 1) * 100 > capacity * load_factor`, resizing at the 70% target (verified by
  `resizes_when_load_factor_exceeded`).
- Key matching is consistent: `FatPointer::eq` compares pointers only, and `is_occupied` uses
  pointer comparison too, so insert and lookup agree on "same key." `find_entry_with_value` stays
  content-based on purpose (it's the interning lookup). Valid because every string — literals and
  `concat` results — is interned (Option A from §4).

### ❌ Open gaps

1. **Insert stops at the first tombstone → duplicate keys + `size` drift.** `find_bucket` uses
   `is_occupied`, which returns `false` for a `TombStone`, so the insert probe *stops* at the first
   tombstone instead of continuing to check whether the key already exists further along. Repro:
   ```
   insert(A); insert(B)   // A and B collide; B probes to the next slot
   delete(A)              // tombstone in A's slot
   insert(B, new)         // stops at the tombstone -> writes a 2nd copy of B; size++ again
   ```
   Fix: implement the insert-probe from §3 (remember first tombstone, keep scanning for a match to
   `Vacant`, then place at the tombstone). The `reinsert_after_delete_does_not_duplicate` test is
   currently failing on purpose until this is fixed. (`delete` shares `find_bucket` and has the same
   flaw for keys past a tombstone — fix together.)

### Related (outside this file) — resolved
- **Computed-string equality** (`common.rs`): fixed. Equality is pointer identity, and `concat` now
  interns its result (plus string literals no longer carry quotes), so `"a" + "b"` yields the same
  interned pointer as the literal `ab` and `==` is `true`. This was design **A** from §4.

---

## 6. Suggested fix order (remaining)

1. Rewrite the insert-probe (§3) so it can never duplicate and it reuses tombstones (gap #1); then
   the `reinsert_after_delete_does_not_duplicate` test goes green. Fix `delete`'s shared use of
   `find_bucket` at the same time.

Each fix should get a regression test (see [`TEST_PLAN.md`](TEST_PLAN.md)); force collisions in
tests by constructing `FatPointer`s with equal `hash` fields but distinct `ptr`/`size`.

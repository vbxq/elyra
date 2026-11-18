# NaN-Boxed Value Representation

This module implements Aelys values using NaN-boxing to pack primitives and object
pointers into a single 64-bit word.

- Integers in -2^47..2^47 are stored inline; larger ones (up to the i64 and u64 ranges) are heap `BoxedInt` objects, compared and hashed by value.
- Floats are stored as raw IEEE-754 bits.
- Tagged values encode int/bool/null/ptr in the NaN payload.

Use `Value::inline_int` for the inline encoding and the VM's integer constructors for any i64.

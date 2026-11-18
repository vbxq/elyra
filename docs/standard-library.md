# Standard Library Reference

All standard library modules live under `std::` paths.

### Auto-registered modules

The safe modules `std::io`, `std::math`, `std::string`, `std::convert`, and
`std::time` are auto-registered at VM startup. You can use their functions
immediately without `needs`. For example, `println("hello")` and
`math::sqrt(16.0)` work out of the box.

Two builtins sit outside every module and need no `needs`: `panic(message)`, of
type `never`, and `assert(condition, message)`. Both stop the program with
`panic: <message>`, the source location, and the stack trace on stderr, and the
runner exits with status 101 (see Panics in the language specification).

You can still use `needs` with an alias if you want a shorter name:

```rust
needs std::math as m
```


```rust
needs std::fs
needs std::net
```

---

## std::io

Console input/output. Auto-registered -- no `needs` required.

### Output Functions

| Function | Description |
|----------|-------------|
| `print(value)` | Print value without newline |
| `println(value)` | Print value with newline; a `Vec`, array, `Map`, tuple, `Option` or `Result` prints as `[1, 2]`, `{"k": 1}`, `(1, "x")`, `Some(3)` |
| `eprint(value)` | Print to stderr, no newline |
| `eprintln(value)` | Print to stderr with newline |
| `flush()` | Flush stdout buffer |
| `eflush()` | Flush stderr buffer |

```rust
println("Hello")         // Hello\n
print("Loading")         // Loading (no newline)
print(".")               // .
println("")              // newline
```

### Input Functions

| Function | Description |
|----------|-------------|
| `readline()` | Read line from stdin as `Option<string>` |
| `read_char()` | Read a single character as `Option<string>` |
| `input(prompt)` | Print prompt and read an `Option<string>` |

```rust
let name = input("What's your name? ")
println("Hello, {name}")
```

### Terminal Control

ANSI escape sequence wrappers. Work on most modern terminals.

| Function | Description |
|----------|-------------|
| `clear_screen()` | Clear the terminal |
| `cursor_home()` | Move cursor to top-left |
| `hide_cursor()` | Hide the cursor |
| `show_cursor()` | Show the cursor |
| `move_cursor(x, y)` | Move cursor to position (1-indexed) |

```rust
clear_screen()
move_cursor(10, 5)
println("Here!")
```

---

## std::math

Math functions and constants. Auto-registered -- no `needs` required.

### Constants

| Name | Value |
|------|-------|
| `PI` | 3.141592653589793 |
| `E` | 2.718281828459045 |
| `TAU` | 6.283185307179586 (2π) |
| `INF` | Positive infinity |
| `NEG_INF` | Negative infinity |

### Basic Math

| Function | Description |
|----------|-------------|
| `abs(x)` | Absolute value |
| `sign(x)` | Sign: -1, 0, or 1 |
| `sqrt(x)` | Square root |
| `cbrt(x)` | Cube root |
| `pow(base, exp)` | Exponentiation |
| `min(a, b)` | Minimum of two values |
| `max(a, b)` | Maximum of two values |
| `clamp(x, min, max)` | Clamp value to range |
| `randint(debut, fin)` | Random integer in range [debut, fin] (inclusive) |

`math::randint`, `sys::random`, and `sys::random_int` consume the same VM-local sequence.

```rust
math::abs(-5)        // 5
math::sqrt(16.0)     // 4.0
math::pow(2, 10)     // 1024
math::clamp(15, 0, 10)  // 10
math::randint(1, 6)  // random int from 1 to 6 (dice roll)
```

### Trigonometry

All functions work in radians.

| Function | Description |
|----------|-------------|
| `sin(x)` | Sine |
| `cos(x)` | Cosine |
| `tan(x)` | Tangent |
| `asin(x)` | Arc sine |
| `acos(x)` | Arc cosine |
| `atan(x)` | Arc tangent |
| `atan2(y, x)` | Two-argument arc tangent |
| `sinh(x)` | Hyperbolic sine |
| `cosh(x)` | Hyperbolic cosine |
| `tanh(x)` | Hyperbolic tangent |

### Angle Conversion

| Function | Description |
|----------|-------------|
| `deg_to_rad(deg)` | Degrees to radians |
| `rad_to_deg(rad)` | Radians to degrees |

### Exponential & Logarithmic

| Function | Description |
|----------|-------------|
| `exp(x)` | e^x |
| `log(x)` | Natural logarithm |
| `log10(x)` | Base-10 logarithm |
| `log2(x)` | Base-2 logarithm |

### Rounding

| Function | Description |
|----------|-------------|
| `floor(x)` | Round down |
| `ceil(x)` | Round up |
| `round(x)` | Round to nearest |
| `trunc(x)` | Truncate toward zero |

The four rounding functions return an `int`. A NaN, an infinity, or a result outside
the 48-bit `int` range is a runtime error, never a saturated or zero value.
`abs` of the smallest `int` (-140737488355328) is an integer overflow error.

### Special

| Function | Description |
|----------|-------------|
| `hypot(x, y)` | sqrt(x² + y²) |
| `fmod(x, y)` | Floating-point modulo |
| `is_nan(x)` | Check if NaN |
| `is_inf(x)` | Check if infinite |
| `is_finite(x)` | Check if finite |

---

## std::string

String manipulation. Strings are UTF-8. Auto-registered -- no `needs` required.

String functions can be called as methods on string values. The qualified `string::func(s, ...)` syntax also still works.

```rust
// Method syntax (preferred)
let n = "hello".len()
let up = "hello".to_upper()

// Qualified syntax (also valid)
let n = string::len("hello")
let up = string::to_upper("hello")
```

### Length

| Method | Description |
|--------|-------------|
| `s.len()` | Length in bytes |
| `s.char_len()` | Length in Unicode characters |

```rust
"café".len()       // 5 (bytes)
"café".char_len()  // 4 (characters)
```

This distinction matters for non-ASCII text.

### Character Access

| Method | Description |
|--------|-------------|
| `s.char_at(i)` | Character at index (empty if out of bounds) |
| `s.byte_at(i)` | Byte at index (-1 if out of bounds) |
| `s.substr(start, len)` | Extract substring by character position |
| `s.chars()` | `Vec<char>` of the characters |
| `s.bytes()` | `Vec<int>` of the UTF-8 bytes |
| `s.to_bytes()` | `Vec<u8>` of the UTF-8 bytes, the input of the binary methods of [`Vec<u8>`](#binary-data-vecu8) |
| `string::from_char(c)` | One-character string from a `char` |
| `s.push(c)` | Append a `char` to a string variable or place with a mutable root (`p.name.push(c)`) |

```rust
"hello".char_at(0)    // "h"
"hello".char_at(10)   // "" (out of bounds)
"hello".substr(1, 3)  // "ell"
"héllo".chars()       // vec!['h', 'é', 'l', 'l', 'o']
"Aé".bytes()          // vec![65, 195, 169]
```

`s[i]` and a for-each over a string yield `char` values; `char_at` keeps
returning a one-character string.

### Case Conversion

| Method | Description |
|--------|-------------|
| `s.to_upper()` | Convert to uppercase |
| `s.to_lower()` | Convert to lowercase |
| `s.capitalize()` | Capitalize first character |

### Search

| Method | Description |
|--------|-------------|
| `s.contains(needle)` | Check if string contains substring |
| `s.starts_with(prefix)` | Check prefix |
| `s.ends_with(suffix)` | Check suffix |
| `s.find(needle)` | First occurrence index (-1 if not found) |
| `s.rfind(needle)` | Last occurrence index |
| `s.count(needle)` | Count occurrences |

Note: `find` and `rfind` return byte positions, not character positions. This can be surprising with Unicode - I might change this in a future version.

### Transformation

| Method | Description |
|--------|-------------|
| `s.replace(old, new)` | Replace all occurrences |
| `s.replace_first(old, new)` | Replace first occurrence |
| `s.reverse()` | Reverse string |
| `s.repeat(n)` | Repeat n times |
| `s.concat(other)` | Concatenate (same as `+`) |

### Whitespace

| Method | Description |
|--------|-------------|
| `s.trim()` | Remove whitespace from both ends |
| `s.trim_start()` | Remove from start |
| `s.trim_end()` | Remove from end |
| `s.pad_left(width, fill: string)` | Pad start to `width` characters with the first character of `fill` |
| `s.pad_right(width, fill: string)` | Pad end to `width` characters with the first character of `fill` |

`fill` is a `string`, not a `char` (`'0'` is E0301): only its first character is
used, an empty `fill` pads with spaces, and a string already `width` characters or
longer is returned unchanged.

```rust
"  hello  ".trim()        // "hello"
"42".pad_left(5, "0")     // "00042"
"42".pad_right(5, "ab")   // "42aaa"
"4242".pad_left(2, "0")   // "4242"
```

### Splitting

| Method | Description |
|--------|-------------|
| `s.split(sep)` | `Vec<string>` of the pieces between separators; an empty `sep` splits into characters |
| `s.lines()` | `Vec<string>` of the lines, without `\n` or `\r\n` terminators |
| `v.join(sep)` | Join a `Vec<string>` with `sep` (a Vec method) |
| `string::join(text, sep)` | Join the lines of a newline-separated string with `sep` |
| `s.line_count()` | Count lines |

```rust
let parts = "a,b,,c".split(",")      // vec!["a", "b", "", "c"]
parts.join("-")                      // "a-b--c"
"".split(",").len()                  // 1
```

### Comparison and building

Strings compare with `<`, `<=`, `>` and `>=` lexicographically by Unicode scalar
value. `s += x` (and `s + x` where `s` is the most recent string built on its
buffer) appends in place in amortised O(1): the result shares the left operand's
buffer when nothing else has extended it, so a loop that appends n times costs
O(n) overall, and every earlier string value keeps its own contents.
A shared buffer counts against `-ae.max-heap` once, for its whole reserved
capacity, for as long as any string that views it is alive: a short string that
once had a long string appended to it keeps that long buffer alive. Appending
past the budget is a clean `out of memory` error at the append.

### Predicates

| Method | Description |
|--------|-------------|
| `s.is_empty()` | Check if empty |
| `s.is_whitespace()` | Only whitespace? |
| `s.is_numeric()` | Only digits? |
| `s.is_alphabetic()` | Only letters? |
| `s.is_alphanumeric()` | Letters and digits only? |

---

## std::convert

Type conversions and introspection. Auto-registered -- no `needs` required.

### Parsing Strings

| Function | Description |
|----------|-------------|
| `parse_int(s)` | Parse string to `Option<int>` |
| `parse_int_radix(s, radix)` | Parse with base 2-36 as `Option<int>` |
| `parse_float(s)` | Parse string to `Option<float>` |
| `parse_bool(s)` | Parse boolean to `Option<bool>` |

`parse_int` accepts an optional sign followed by decimal digits or a `0x`, `0o`, or
`0b` prefix, with surrounding whitespace ignored. Anything else is `None`: it never
panics and never truncates, so `"3.9"`, `"1e3"`, `"-2.5"`, `"nan"`, and a value
outside the 48-bit `int` range (-140737488355328 to 140737488355327) all give
`None`. `parse_int_radix` follows the same rules for its base.

```rust
match convert::parse_int("42") {
    Some(value) => value,
    None => 0,
}
```

### Type Conversion

| Function | Description |
|----------|-------------|
| `to_string(x)` | Render a `Display` value as a string, aggregates in the form described under Display in the language spec |
| `to_int(x)` | Convert to int |
| `to_float(x)` | Convert to float |
| `to_bool(x)` | Convert to bool (truthiness) |

`to_int` truncates a float toward zero; a NaN, an infinity, or a float outside the
48-bit range is a runtime error. A string passed to `to_int` must be an integer
under the `parse_int` rules, otherwise `to_int` is a runtime error (`to_int("3.9")`
is an error, not `3`).

`.to_string()` can be called as a method on a scalar, a `string` or a built-in
aggregate; an aggregate holding a struct, an enum or a closure is E0338, as for `println`.
On a struct or an enum it calls the type's own `to_string` method when it declares one,
otherwise its `Display` impl, otherwise it is E0338. Inside a generic body (`fn f<T>(t: T)`,
`impl<T> Holder<T>`) the choice is made for each concrete type the body is specialized
with, so `t.to_string()` never prints a raw struct:

```rust
(42).to_string()      // "42"
true.to_string()      // "true"
(3.14).to_string()    // "3.14"
vec!["a"].to_string() // "[\"a\"]"
```

### Numeric Formatting

| Function | Description |
|----------|-------------|
| `to_hex(n)` | Int to hexadecimal string |
| `to_binary(n)` | Int to binary string |
| `to_octal(n)` | Int to octal string |
| `to_radix(n, radix)` | Int to string in given radix |

```rust
convert::to_hex(255)      // "ff"
convert::to_binary(10)    // "1010"
convert::to_radix(100, 7) // "202"
```

### Character Conversion

| Function | Description |
|----------|-------------|
| `ord(char)` | Character to Unicode code point |
| `chr(code)` | Code point to character |

```rust
convert::ord("A")    // 65
convert::chr(65)     // "A"
convert::chr(0x1F600)  // "😀"
```

### Type Checking

| Function | Description |
|----------|-------------|
| `type_of(x)` | Get type name as string |
| `is_int(x)` | Check if int |
| `is_float(x)` | Check if float |
| `is_string(x)` | Check if string |
| `is_bool(x)` | Check if bool |
| `is_function(x)` | Check if function |

```rust
convert::type_of(42)        // "int"
convert::type_of("hello")   // "string"
convert::is_int(42)         // true
```

---

## std::time

Time operations and measurement. Auto-registered -- no `needs` required.

### Current Time

| Function | Description |
|----------|-------------|
| `now()` | Unix timestamp in seconds (float) |
| `now_ms()` | Timestamp in milliseconds |
| `now_us()` | Timestamp in microseconds |
| `now_ns()` | Timestamp in nanoseconds |

### High-Precision Timers

For measuring elapsed time accurately:

| Function | Description |
|----------|-------------|
| `timer()` | Create timer, returns handle |
| `elapsed(h)` | Seconds since timer created |
| `elapsed_ms(h)` | Milliseconds elapsed (int) |
| `elapsed_us(h)` | Microseconds elapsed (int) |
| `reset(h)` | Reset timer to now |

```rust
let t = time::timer()
// ... do work ...
let ms = time::elapsed_ms(t)
println("Took {ms}ms")
```

### Sleep

| Function | Description |
|----------|-------------|
| `sleep(ms)` | Sleep for milliseconds |
| `sleep_us(us)` | Sleep for microseconds |

### Date/Time Components

These return UTC time. Local timezone support isn't implemented yet.

| Function | Description |
|----------|-------------|
| `year()` | Current year |
| `month()` | Month (1-12) |
| `day()` | Day of month (1-31) |
| `hour()` | Hour (0-23) |
| `minute()` | Minute (0-59) |
| `second()` | Second (0-59) |
| `weekday()` | Day of week (0=Sunday, 6=Saturday) |
| `yearday()` | Day of year (1-366) |

### Formatting

| Function | Description |
|----------|-------------|
| `format(fmt)` | Format current time with pattern |
| `iso()` | ISO 8601 format (YYYY-MM-DDTHH:MM:SSZ) |
| `date()` | Date only (YYYY-MM-DD) |
| `time_str()` | Time only (HH:MM:SS) |

Format specifiers:
- `%Y` - 4-digit year
- `%y` - 2-digit year
- `%m` - month (01-12)
- `%d` - day (01-31)
- `%H` - hour (00-23)
- `%M` - minute (00-59)
- `%S` - second (00-59)
- `%a` - weekday name (Sun, Mon, ...)
- `%b` - month name (Jan, Feb, ...)
- `%%` - literal %

```rust
time::format("%Y-%m-%d %H:%M:%S")  // "2024-01-15 14:30:45"
time::iso()                        // "2024-01-15T14:30:45Z"
```

---

## std::fs

File system operations.

```rust
needs std::fs
```

### Reading Files

| Function | Description |
|----------|-------------|
| `read_text(path)` | Read entire file as `Result<string, string>` |
| `open(path, mode)` | Open file as `Result<int, string>` |
| `read(handle)` | Read as `Result<string, string>` |
| `read_line(handle)` | Read as `Result<Option<string>, string>` |
| `read_bytes(path)` | Read a whole file as `Result<Vec<u8>, string>`; a file larger than 256 MiB (268435456 bytes), or an endless one such as `/dev/zero`, is an `Err` after reading at most one byte past the limit |
| `read_chunk(handle, n)` | Read up to `n` bytes (at most 16 MiB) from a handle as `Result<Vec<u8>, string>`; fewer than `n` only at end of file |
| `close(handle)` | Close as `Result<unit, string>` |

Modes: `"r"` (read), `"w"` (write), `"a"` (append), `"rw"` (read+write).

`read`, `read_line` and `read_chunk` share one buffered reader per handle, so they can be mixed: each continues where the previous one stopped.

```rust
let content = match fs::read_text("config.txt") {
    Ok(value) => value,
    Err(error) => return error,
}

// Handle-based way
let f = match fs::open("data.txt", "r") {
    Ok(handle) => handle,
    Err(error) => return error,
}
while true {
    match fs::read_line(f) {
        Ok(Some(line)) => println(line),
        Ok(None) => break,
        Err(error) => return error,
    }
}
let _ = fs::close(f)
```

### Writing Files

| Function | Description |
|----------|-------------|
| `write_text(path, content)` | Write string to file (overwrite) |
| `append_text(path, content)` | Append string to file |
| `write(handle, data)` | Write a string to a handle |
| `write_line(handle, line)` | Write with newline |
| `write_bytes(path, &v)` | Write the bytes of a `Vec<u8>` to `path` (overwrite) as `Result<unit, string>` |
| `write_chunk(handle, &v)` | Write the bytes of a `Vec<u8>` to a handle as `Result<int, string>` (bytes written) |

Strings are UTF-8, so a byte of 0x80 or more cannot pass through `write` or
`write_text` unchanged. Binary files go through a `Vec<u8>`, borrowed with `&`
for the call:

```rust
needs std::fs
let buffer: Vec<u8> = vec![0xC8, 0xFF]
let _ = fs::write_bytes("out.bin", &buffer)
let back = fs::read_bytes("out.bin").unwrap()
```

```rust
let _ = fs::write_text("output.txt", "Hello, file!")
let _ = fs::append_text("log.txt", "New entry\n")
```

### File Information

| Function | Description |
|----------|-------------|
| `exists(path)` | Check if path exists |
| `is_file(path)` | Check if regular file |
| `is_dir(path)` | Check if directory |
| `size(path)` | File size in bytes as `Result<int, string>` |

### Directory Operations

| Function | Description |
|----------|-------------|
| `mkdir(path)` | Create directory |
| `mkdir_all(path)` | Create directory and parents |
| `rmdir(path)` | Remove empty directory |
| `readdir(path)` | List directory (newline-separated) |

### File Management

| Function | Description |
|----------|-------------|
| `delete(path)` | Delete file |
| `rename(from, to)` | Rename or move |
| `copy(from, to)` | Copy file |

### Path Utilities

| Function | Description |
|----------|-------------|
| `basename(path)` | Get file name |
| `dirname(path)` | Get directory part |
| `extension(path)` | Get file extension |
| `join(base, path)` | Join paths safely |
| `absolute(path)` | Convert to absolute path |

`join` is secure against path traversal - it won't let `..` escape the base directory.

```rust
fs::join("/home/user", "../../etc/passwd")  // stays within /home/user
```

---

## std::net

Network operations.

```rust
needs std::net
```

### TCP Client

| Function | Description |
|----------|-------------|
| `connect(host, port)` | Connect to server (30s timeout), returns handle |
| `connect_timeout(host, port, ms)` | Connect with custom timeout in milliseconds, returns handle |
| `send(handle, data)` | Send data |
| `recv(handle)` | Receive available data |
| `recv_bytes(handle, max)` | Receive up to max bytes |
| `recv_line(handle)` | Receive one line |
| `close(handle)` | Close connection |

```rust
let sock = net::connect("example.com", 80)
let response = net::recv(sock)
println(response)
let _ = net::close(sock)
```

### TCP Server

| Function | Description |
|----------|-------------|
| `listen(host, port)` | Start listening, returns handle |
| `accept(handle)` | Accept connection, returns new handle |

```rust
let server = net::listen("0.0.0.0", 8080)
println("Listening on port 8080")

while true {
    let client = net::accept(server)
    let request = net::recv_line(client)
    let _ = net::send(client, "HTTP/1.0 200 OK\r\n\r\nHello!")
    let _ = net::close(client)
}
```

### UDP

| Function | Description |
|----------|-------------|
| `udp_bind(host, port)` | Bind a UDP socket, returns handle |
| `udp_send_to(handle, data, addr)` | Send datagram to `"host:port"` |
| `udp_recv_from(handle, max)` | Receive datagram (up to max bytes) |
| `udp_connect(handle, host, port)` | Connect UDP socket to fixed remote |
| `udp_send(handle, data)` | Send on connected UDP socket |
| `udp_recv(handle, max)` | Receive on connected UDP socket |
| `udp_set_broadcast(handle, enabled)` | Enable/disable broadcast |

```rust
// Connectionless (send_to / recv_from)
let sock = net::udp_bind("0.0.0.0", 0)
let _ = net::udp_send_to(sock, "hello", "127.0.0.1:9000")
let data = net::udp_recv_from(sock, 1024)
let _ = net::close(sock)

// Connected mode
let sock = net::udp_bind("0.0.0.0", 0)
let _ = net::udp_connect(sock, "127.0.0.1", 9000)
let _ = net::udp_send(sock, "hello")
let data = net::udp_recv(sock, 1024)
let _ = net::close(sock)
```

`close`, `set_timeout`, and `local_addr` also work with UDP handles.

### Socket Options

| Function | Description |
|----------|-------------|
| `set_timeout(handle, ms)` | Set read/write timeout (0 to disable) |
| `set_nodelay(handle, bool)` | Enable/disable Nagle's algorithm |
| `shutdown(handle, how)` | Partial shutdown ("read", "write", "both") |
| `local_addr(handle)` | Get local address |
| `peer_addr(handle)` | Get peer address |

---

## std::sys

System information.

```rust
needs std::sys
```

| Function | Description |
|----------|-------------|
| `arg_count()` | Number of program arguments, as `int` |
| `arg(index)` | Program argument `index` (0-based) as `Option<string>`; `None` when `index` is negative or past the end |
| `args()` | Every program argument joined with `"\n"`, as one `string` (`""` when there is none) |
| `platform()` | OS name ("linux", "macos", "windows") |
| `arch()` | CPU architecture ("x86_64", "aarch64", etc.) |
| `cwd()` | Current directory as `Result<string, string>` |
| `set_cwd(path)` | Change directory, returning `Result<unit, string>` |
| `exec(command)` | Run a shell command, returning `Result<int, string>` |
| `exec_output(command)` | Capture stdout as `Result<string, string>` |
| `exec_args(program, args)` | Run without a shell as `Result<int, string>` |
| `exec_args_output(program, args)` | Capture stdout without a shell as `Result<string, string>` |
| `random()` | Random float in [0, 1) |
| `random_int(min, max)` | Random integer in [min, max] as `Result<int, string>` |
| `random_seed(seed)` | Restart the VM-local random sequence from a seed |
| `random_state()` | Return the current random state |
| `random_set_state(state)` | Restore a previously returned random state |
| `gc_max_pause_us()` | Longest garbage collection pause since the VM started, in microseconds |
| `gc_window_max_pause_us()` | Longest pause since the previous call of this function, in microseconds |
| `gc_p99_pause_us()` | 99th percentile of the pauses, in microseconds (10 µs resolution) |
| `gc_max_pause_cpu_us()` | Longest pause counted in thread CPU time, which leaves out time the OS spent running other processes |
| `gc_total_pause_us()` | Sum of all pauses, in microseconds |
| `gc_pause_count()` | Number of pauses (minor collections and major slices) |
| `gc_minor_collections()` | Completed minor collections |
| `gc_major_collections()` | Completed major collections |
| `gc_heap_bytes()` | Estimated bytes currently allocated on the heap |
| `gc_live_objects()` | Objects currently on the heap, garbage not yet collected included |
| `gc_allocations()` | Objects allocated since the VM started |
| `gc_stats()` | One-line summary of the figures above, as a string |
| `gc_collect()` | Run a full collection now (minor, then a complete major); its pause grows with the heap |

The program arguments are the command-line words after the script path:
`aelys script.aelys a "b c"` gives `arg_count()` 2, `arg(0)` `Some("a")` and
`arg(1)` `Some("b c")`. The script path itself is not an argument. The CLI reads its
own flags (`-O<level>`, `-ae.<k>=<v>`, `-h`, the warning flags and the others listed by
`aelys --help`) anywhere on the line, also after the script path, so those words never
reach the program; any other word, `--` and words starting with `-` included, is passed
through in order. A program run through the embedding API sees the arguments its host
set, and none by default.

```rust
needs std::sys

if sys::arg_count() == 0 {
    println("usage: tool <file>")
} else {
    println("first argument: {sys::arg(0).unwrap_or("")}")
}
```

The command and directory functions return `Err(message)` when the operating
system operation cannot be started or completed. A nonzero child exit status is
an `Ok(status)` value, not a runtime exception.

```rust
println("Running on {sys::platform()} {sys::arch()}")
// "Running on linux x86_64"
```

---

## Binary data (`Vec<u8>`)

There is no byte-buffer module: binary data is an ordinary `Vec<u8>`, collected
by the garbage collector like any other Vec. `v[i]` reads and writes one byte as a
`u8`; the methods below decode and encode wider values at a byte offset. They exist
only on `Vec<u8>` (another element type has no `read_u16_le`), and every `write_*`
needs a mutable receiver (E0318). `needs std::bytes` is E0480 and a `bytes::name`
call is E0481; both name the replacement.

```rust
let mut v: Vec<u8> = vec![0; 16]
let header = "PWAD".to_bytes()
let file = fs::read_bytes("doom.wad").unwrap()
```

| Method | Result | Meaning |
|--------|--------|---------|
| `v.read_u8(off)`, `v.read_i8(off)` | `int` | one byte, unsigned or signed |
| `v.read_u16_le(off)`, `v.read_u16_be(off)` | `int` | 16-bit unsigned, little or big endian |
| `v.read_i16_le(off)`, `v.read_i16_be(off)` | `int` | 16-bit signed |
| `v.read_u32_le(off)`, `v.read_u32_be(off)` | `int` | 32-bit unsigned |
| `v.read_i32_le(off)`, `v.read_i32_be(off)` | `int` | 32-bit signed |
| `v.read_u64_le(off)`, `v.read_u64_be(off)` | `int` | 64-bit unsigned; a value `int` cannot hold traps |
| `v.read_i64_le(off)`, `v.read_i64_be(off)` | `int` | 64-bit signed; a value `int` cannot hold traps |
| `v.read_f32_le(off)`, `v.read_f32_be(off)` | `float` | IEEE 754 single, widened exactly |
| `v.read_f64_le(off)`, `v.read_f64_be(off)` | `float` | IEEE 754 double |
| `v.write_u8(off, x)` ... `v.write_i64_be(off, x)` | `unit` | the same widths and byte orders as the reads; `x` is an `int` |
| `v.write_f32_le(off, x)` ... `v.write_f64_be(off, x)` | `unit` | `x` is a `float`; `f32` rounds to nearest |
| `v.to_utf8()` | `Option<string>` | the bytes as a string, `None` when they are not valid UTF-8 |
| `v.copy_within(start, end, dest)` | `unit` | copy the bytes `start..end` to `dest`, overlapping ranges included |
| `s.to_bytes()` | `Vec<u8>` | the UTF-8 bytes of the string `s` |

Every offset is checked: an access that does not fit traps with
`read_u32_le: bytes 13..17 are out of bounds for length 16`, which names the method,
the byte range and the length (a negative offset is shown as such). A `write_*`
whose value does not fit its width traps with
`write_u16_le: value 65536 does not fit in u16`; signed and unsigned writes accept
exactly the range of their type, so `write_u8(off, -1)` traps and `write_i8(off, -1)`
stores 255. A 64-bit read whose value `int` cannot represent traps with
`read_u64_le: value 18446744073709551615 does not fit in int`.

Slices, `fill`, `len`, `resize`, `extend` and the other Vec methods work as on any
Vec: `v[4..8].to_utf8()` decodes four bytes, `v.fill(0)` clears a frame.

```rust
fn parse_header(packet: Vec<u8>) -> string {
    let magic = packet.read_u32_be(0)
    let length = packet.read_u16_le(4)
    let name = match packet[8..12].to_utf8() {
        Some(text) => text,
        None => "?",
    }
    "{magic} {length} {name}"
}

let mut packet: Vec<u8> = vec![0; 12]
packet.write_u32_be(0, 0xDEADBEEF)
packet.write_u16_le(4, 1024)
packet.write_i16_be(6, -100)
let name = "Meow".to_bytes()
for i in 0..4 { packet[8 + i] = name[i] }
```

A `Vec<u8>` is handed to a native function as `&v`, or `&mut v` when the native
writes into it; see [Native modules](native-modules.md). In the current VM a
`Vec<u8>` stores each element in an 8-byte integer slot, like `Vec<int>`, so a
file read with `fs::read_bytes` takes eight bytes of heap per byte of file.

---

That's all for now. More modules might be added in future versions!

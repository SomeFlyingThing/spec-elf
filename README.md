# spec-elf

`spec-elf` builds several x86-64 variants of a project and combines them with a small launcher into one ELF executable. At runtime, the launcher detects the host CPU level and starts the best compatible payload, this only happens in the first run, then it is replaced with the best version for your machine.

## Requirements

- Linux on x86-64
- Rust and Cargo (to build `spec-elf` and Rust projects)
- The relevant project toolchain: `gcc`, `g++`, `cmake`, or `zig`

## Build

```bash
cargo build --release
```

The resulting launcher is `target/release/spec-elf`.

## Package a project

Pass the project directory explicitly. Use `.` for the current directory:

```bash
cd /path/to/project
/path/to/spec-elf/target/release/spec-elf .

# or
/path/to/spec-elf/target/release/spec-elf /path/to/project
```

The project language is inferred from source-file extensions. The tool supports C, C++, Rust, and Zig projects. It writes intermediate binaries to `build/` and produces a packed executable named `spec-elf` in the project directory.

For C and C++, projects with `CMakeLists.txt` are built with CMake; otherwise all matching source files are passed directly to `gcc` or `g++`. Rust projects are built with Cargo. Zig projects currently build the first `.zig` source file found.

## CPU variants

The package includes builds for the generic x86-64 baseline plus x86-64-v2, v3, and v4. It also includes a `native` build, selected only when the launcher recognizes the exact CPU it was built for. If no native match is available, the launcher selects the highest supported x86-64 level.

## Development

```bash
cargo test
cargo run -- --help
```

`spec-elf` is experimental. Packaging replaces the output executable in the target project directory, so run it in a disposable directory when trying it out.

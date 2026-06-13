# pokemem

A lightweight command-line game memory scanner and editor for Windows, written in Rust. Think of it as a minimal Cheat Engine you can cross-compile from Linux.

Attach to a running game process, search for a value (like your current gold), change it in-game, rescan to narrow down the address, then write whatever value you want.

## Building

### On Windows

```
cargo build --release
```

### Cross-compile from Linux

```
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

The binary will be at `target/x86_64-pc-windows-gnu/release/pokemem.exe` (~1.2 MB).

## Usage

**Run as Administrator** (required to read/write other processes' memory).

```
pokemem.exe
```

### Example: editing gold in a strategy game

```
=== Memory Scanner ===

Enter process name (or part of it) to search, or PID: game

Matching processes:
  [0] game.exe (PID: 12345)
Auto-selecting the only match.

Attaching to PID 12345...
Attached.

Enter the current value to search for (i32): 500
Scanning 1842.3 MB across 4917 regions...
Found 31474 matches.

Change the value in-game, then enter the new value (or 'done' to stop narrowing): 485
Narrowed to 3 matches.
  [0] 0x1A2B3C40 = 485
  [1] 0x2C4D5E60 = 485
  [2] 0x3F607180 = 485

Change the value in-game, then enter the new value (or 'done' to stop narrowing): 470
Narrowed to 1 matches.
  [0] 0x1A2B3C40 = 470
Exact match found!

1 candidate address(es):
  [0] 0x1A2B3C40 = 470

Enter index to edit (or 'all' for all, 'quit' to exit): 0
Enter new value: 999999
  Wrote 999999 to 0x1A2B3C40

Enter index to edit (or 'all' for all, 'quit' to exit): quit
Done.
```

### Step by step

1. **Find the process** -- Type part of the game's exe name (e.g. `civ`, `total`, `crusader`) or a PID directly. If multiple processes match, pick one from the list.

2. **Initial scan** -- Enter the current in-game value you want to find (e.g. your gold count `500`). The tool scans all readable memory for matching 32-bit integers. Expect thousands of hits.

3. **Narrow down** -- Go back to the game and change the value naturally (spend gold, earn gold, etc.). Then enter the new value. The tool keeps only addresses that now hold the new value. Repeat until you have 1-3 candidates.

4. **Edit** -- Pick an address by index (or type `all` to write to every candidate) and set your desired value. Type `quit` when done.

## How it works

- Uses Win32 APIs: `OpenProcess`, `VirtualQueryEx`, `ReadProcessMemory`, `WriteProcessMemory`
- Enumerates all committed memory regions with readable protection flags
- Scans for little-endian `i32` values (covers most game integers: gold, HP, ammo, etc.)
- Rescans only previously matched addresses for fast narrowing
- Single file, no runtime dependencies beyond the Windows API

## Requirements

- Windows x86-64
- Administrator privileges
- The target game must store the value as a 32-bit integer in memory (most games do)

## Limitations

- Only scans for `i32` (4-byte signed integer) values
- No support for floating-point, string, or multi-byte pattern searches
- No pointer chain resolution (address may change between game restarts)
- Some games with anti-cheat (EAC, BattlEye, etc.) will block process access -- this tool is meant for singleplayer games without anti-cheat

## License

MIT

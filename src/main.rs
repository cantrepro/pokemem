#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("This tool only runs on Windows. Cross-compile with:");
    eprintln!("  cargo build --release --target x86_64-pc-windows-gnu\n");
}

#[cfg(target_os = "windows")]
fn main() {
    if let Err(e) = win::run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "windows")]
mod win {
    use std::io::{self, BufRead, Write};
    use std::mem;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::Diagnostics::Debug::*;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    use windows_sys::Win32::System::Memory::*;
    use windows_sys::Win32::System::Threading::*;

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    struct Process {
        handle: HANDLE,
    }

    impl Process {
        fn open(pid: u32) -> Result<Self> {
            let handle = unsafe {
                OpenProcess(
                    PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION | PROCESS_QUERY_INFORMATION,
                    0,
                    pid,
                )
            };
            if handle.is_null() {
                return Err(format!("Failed to open process {pid}. Run as Administrator.").into());
            }
            Ok(Self { handle })
        }

        fn readable_regions(&self) -> Vec<(usize, usize)> {
            let mut regions = Vec::new();
            let mut addr: usize = 0;
            loop {
                let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { mem::zeroed() };
                let ret = unsafe {
                    VirtualQueryEx(
                        self.handle,
                        addr as *const _,
                        &mut mbi,
                        mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                    )
                };
                if ret == 0 {
                    break;
                }
                // Only scan committed, readable, private/mapped memory (skip guards, images, etc. optionally)
                if mbi.State == MEM_COMMIT
                    && (mbi.Protect == PAGE_READWRITE
                        || mbi.Protect == PAGE_EXECUTE_READWRITE
                        || mbi.Protect == PAGE_READONLY
                        || mbi.Protect == PAGE_EXECUTE_READ
                        || mbi.Protect == PAGE_WRITECOPY
                        || mbi.Protect == PAGE_EXECUTE_WRITECOPY)
                {
                    regions.push((mbi.BaseAddress as usize, mbi.RegionSize));
                }
                addr = mbi.BaseAddress as usize + mbi.RegionSize;
                if addr == 0 {
                    break; // wrapped around
                }
            }
            regions
        }

        fn read_memory(&self, addr: usize, buf: &mut [u8]) -> bool {
            let mut bytes_read: usize = 0;
            let ok = unsafe {
                ReadProcessMemory(
                    self.handle,
                    addr as *const _,
                    buf.as_mut_ptr() as *mut _,
                    buf.len(),
                    &mut bytes_read,
                )
            };
            ok != 0 && bytes_read == buf.len()
        }

        fn write_memory(&self, addr: usize, data: &[u8]) -> bool {
            let mut bytes_written: usize = 0;
            let ok = unsafe {
                WriteProcessMemory(
                    self.handle,
                    addr as *mut _,
                    data.as_ptr() as *const _,
                    data.len(),
                    &mut bytes_written,
                )
            };
            ok != 0 && bytes_written == data.len()
        }
    }

    impl Drop for Process {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.handle) };
        }
    }

    fn list_processes() -> Result<Vec<(u32, String)>> {
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snap == INVALID_HANDLE_VALUE {
            return Err("Failed to create process snapshot".into());
        }

        let mut procs = Vec::new();
        let mut entry: PROCESSENTRY32 = unsafe { mem::zeroed() };
        entry.dwSize = mem::size_of::<PROCESSENTRY32>() as u32;

        if unsafe { Process32First(snap, &mut entry) } != 0 {
            loop {
                let name = entry.szExeFile.iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8 as char)
                    .collect::<String>();
                procs.push((entry.th32ProcessID, name));
                if unsafe { Process32Next(snap, &mut entry) } == 0 {
                    break;
                }
            }
        }
        unsafe { CloseHandle(snap) };
        Ok(procs)
    }

    fn prompt(msg: &str) -> String {
        print!("{msg}");
        io::stdout().flush().unwrap();
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line).unwrap();
        line.trim().to_string()
    }

    fn scan_value(proc: &Process, value: i32) -> Vec<usize> {
        let regions = proc.readable_regions();
        let needle = value.to_le_bytes();
        let mut matches = Vec::new();

        let total_bytes: usize = regions.iter().map(|(_, sz)| *sz).sum();
        println!("Scanning {:.1} MB across {} regions...",
            total_bytes as f64 / (1024.0 * 1024.0), regions.len());

        for (base, size) in &regions {
            let mut buf = vec![0u8; *size];
            if !proc.read_memory(*base, &mut buf) {
                continue;
            }
            for offset in 0..size.saturating_sub(3) {
                if buf[offset..offset + 4] == needle {
                    matches.push(base + offset);
                }
            }
        }
        matches
    }

    fn rescan_value(proc: &Process, candidates: &[usize], new_value: i32) -> Vec<usize> {
        let needle = new_value.to_le_bytes();
        let mut matches = Vec::new();
        let mut buf = [0u8; 4];
        for &addr in candidates {
            if proc.read_memory(addr, &mut buf) && buf == needle {
                matches.push(addr);
            }
        }
        matches
    }

    pub fn run() -> Result<()> {
        println!("=== Memory Scanner ===\n");

        // Step 1: Find and select process
        let input = prompt("Enter process name (or part of it) to search, or PID: ");

        let pid: u32 = if let Ok(pid) = input.parse() {
            pid
        } else {
            let procs = list_processes()?;
            let filter = input.to_lowercase();
            let matched: Vec<_> = procs.iter()
                .filter(|(_, name)| name.to_lowercase().contains(&filter))
                .collect();

            if matched.is_empty() {
                return Err(format!("No process matching '{input}' found.").into());
            }

            println!("\nMatching processes:");
            for (i, (pid, name)) in matched.iter().enumerate() {
                println!("  [{i}] {name} (PID: {pid})");
            }

            if matched.len() == 1 {
                println!("Auto-selecting the only match.");
                matched[0].0
            } else {
                let idx: usize = prompt("\nSelect process index: ").parse()?;
                matched.get(idx).ok_or("Invalid index")?.0
            }
        };

        println!("\nAttaching to PID {pid}...");
        let proc = Process::open(pid)?;
        println!("Attached.\n");

        // Step 2: Initial scan
        let value: i32 = prompt("Enter the current value to search for (i32): ").parse()?;
        let mut candidates = scan_value(&proc, value);
        println!("Found {} matches.\n", candidates.len());

        if candidates.is_empty() {
            return Err("No matches found. Make sure the value and process are correct.".into());
        }

        // Step 3: Rescan loop - change value in-game, then rescan
        loop {
            let input = prompt("Change the value in-game, then enter the new value (or 'done' to stop narrowing): ");
            if input == "done" {
                break;
            }
            let new_value: i32 = match input.parse() {
                Ok(v) => v,
                Err(_) => { println!("Invalid number."); continue; }
            };
            candidates = rescan_value(&proc, &candidates, new_value);
            println!("Narrowed to {} matches.", candidates.len());
            if candidates.len() <= 20 {
                for (i, &addr) in candidates.iter().enumerate() {
                    let mut buf = [0u8; 4];
                    let val = if proc.read_memory(addr, &mut buf) {
                        i32::from_le_bytes(buf)
                    } else { 0 };
                    println!("  [{i}] 0x{addr:X} = {val}");
                }
            }
            if candidates.is_empty() {
                return Err("All candidates eliminated. Try again from scratch.".into());
            }
            if candidates.len() == 1 {
                println!("Exact match found!");
                break;
            }
            println!();
        }

        // Step 4: Write new value
        loop {
            println!("\n{} candidate address(es):", candidates.len());
            for (i, &addr) in candidates.iter().enumerate() {
                let mut buf = [0u8; 4];
                let val = if proc.read_memory(addr, &mut buf) {
                    i32::from_le_bytes(buf)
                } else { 0 };
                println!("  [{i}] 0x{addr:X} = {val}");
            }

            let input = prompt("\nEnter index to edit (or 'all' for all, 'quit' to exit): ");
            if input == "quit" {
                break;
            }

            let indices: Vec<usize> = if input == "all" {
                (0..candidates.len()).collect()
            } else {
                match input.parse::<usize>() {
                    Ok(i) if i < candidates.len() => vec![i],
                    _ => { println!("Invalid index."); continue; }
                }
            };

            let new_val: i32 = match prompt("Enter new value: ").parse() {
                Ok(v) => v,
                Err(_) => { println!("Invalid number."); continue; }
            };

            let data = new_val.to_le_bytes();
            for idx in indices {
                let addr = candidates[idx];
                if proc.write_memory(addr, &data) {
                    println!("  Wrote {new_val} to 0x{addr:X}");
                } else {
                    println!("  FAILED to write to 0x{addr:X}");
                }
            }
        }

        println!("Done.");
        Ok(())
    }
}

// Desktop-LuaJIT oracle: runs a Lua chunk in PoB's runtime/lua51.dll (MSVC/UCRT LuaJIT) and prints its string result.
//   luajit-win <file.lua>
// `POB_LUA51_EXPECT=<path of the lua51.dll that must be loaded>`: the DLL is found through the usual search (the caller puts
// upstream's runtime/ first on PATH); a wrong path once picked another LuaJIT silently, so with this set a different DLL is an error.
// The chunk is loaded under its file name (`@<name>`), not as a string: functions defined in it then carry a short `source`.
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;

#[link(name = "lua51", kind = "raw-dylib")]
unsafe extern "C" {
    fn luaL_newstate() -> *mut c_void;
    fn luaL_openlibs(l: *mut c_void);
    fn luaL_loadbuffer(l: *mut c_void, buf: *const c_char, size: usize, name: *const c_char) -> c_int;
    fn lua_pcall(l: *mut c_void, nargs: c_int, nres: c_int, errf: c_int) -> c_int;
    fn lua_tolstring(l: *mut c_void, idx: c_int, len: *mut usize) -> *const c_char;
}

#[link(name = "kernel32", kind = "raw-dylib")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetModuleFileNameW(module: *mut c_void, buf: *mut u16, size: u32) -> u32;
}

/// The file the process loaded as `lua51.dll`.
fn loaded_lua51() -> Option<String> {
    let name: Vec<u16> = "lua51.dll\0".encode_utf16().collect();
    let mut buf = vec![0u16; 32768];
    unsafe {
        let h = GetModuleHandleW(name.as_ptr());
        if h.is_null() {
            return None;
        }
        let n = GetModuleFileNameW(h, buf.as_mut_ptr(), buf.len() as u32) as usize;
        (n > 0).then(|| String::from_utf16_lossy(&buf[..n]))
    }
}

fn same_file(a: &str, b: &str) -> bool {
    match (Path::new(a).canonicalize(), Path::new(b).canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: luajit-win <file.lua>");
    let code = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let name = Path::new(&path).file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
    let chunk_name = CString::new(format!("@{name}")).expect("chunk name");
    unsafe {
        let l = luaL_newstate();
        if let Ok(want) = std::env::var("POB_LUA51_EXPECT") {
            let got = loaded_lua51().unwrap_or_default();
            if !same_file(&got, &want) {
                eprintln!("luajit-win: loaded lua51.dll is {got:?}, POB_LUA51_EXPECT is {want:?}");
                std::process::exit(2);
            }
        }
        luaL_openlibs(l);
        if luaL_loadbuffer(l, code.as_ptr() as *const c_char, code.len(), chunk_name.as_ptr()) != 0 || lua_pcall(l, 0, 1, 0) != 0 {
            eprintln!("lua error: {}", CStr::from_ptr(lua_tolstring(l, -1, std::ptr::null_mut())).to_string_lossy());
            std::process::exit(1);
        }
        let mut n = 0usize;
        let p = lua_tolstring(l, -1, &mut n);
        let bytes = std::slice::from_raw_parts(p as *const u8, n);
        print!("{}", String::from_utf8_lossy(bytes));
    }
}

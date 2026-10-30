// Desktop-LuaJIT oracle: runs a Lua chunk in PoB's runtime/lua51.dll (MSVC/UCRT LuaJIT) and prints its string result.
use std::ffi::{CStr, CString, c_char, c_int, c_void};
#[link(name = "lua51", kind = "raw-dylib")]
unsafe extern "C" {
    fn luaL_newstate() -> *mut c_void;
    fn luaL_openlibs(l: *mut c_void);
    fn luaL_loadstring(l: *mut c_void, s: *const c_char) -> c_int;
    fn lua_pcall(l: *mut c_void, nargs: c_int, nres: c_int, errf: c_int) -> c_int;
    fn lua_tolstring(l: *mut c_void, idx: c_int, len: *mut usize) -> *const c_char;
}
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let code = std::fs::read_to_string(path).unwrap();
    let c = CString::new(code).unwrap();
    unsafe {
        let l = luaL_newstate();
        luaL_openlibs(l);
        if luaL_loadstring(l, c.as_ptr()) != 0 || lua_pcall(l, 0, 1, 0) != 0 {
            eprintln!("lua error: {}", CStr::from_ptr(lua_tolstring(l, -1, std::ptr::null_mut())).to_string_lossy());
            std::process::exit(1);
        }
        let mut n = 0usize;
        let p = lua_tolstring(l, -1, &mut n);
        let bytes = std::slice::from_raw_parts(p as *const u8, n);
        print!("{}", String::from_utf8_lossy(bytes));
    }
}

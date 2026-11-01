use std::os::unix::process::CommandExt;
extern "C" {
    fn readlink(path: *const i8, buf: *mut i8, n: usize) -> isize;
    #[link_name = "write"] fn o_write(f: i32, b: *const u8, n: usize) -> isize;
}
fn main() {
    let log = std::fs::File::create("/tmp/s0_fd2-target.txt").unwrap();
    let out = std::fs::File::create("/tmp/s0_fd2-result.txt").unwrap();
    let mut cmd = std::process::Command::new("/bin/true");
    cmd.stderr(log);
    cmd.stdout(out);
    unsafe {
        cmd.pre_exec(|| {
            let mut b = [0u8; 256];
            let link = b"/proc/self/fd/2\0";
            let n = readlink(link.as_ptr() as *const i8, b.as_mut_ptr() as *mut i8, 255);
            o_write(1, b.as_ptr(), if n > 0 { n as usize } else { 0 });
            Ok(())
        });
    }
    let _ = cmd.status();
}
